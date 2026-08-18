// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! System Media Transport Controls.
//!
//! Structurally follows souvlaki's SMTC backend, but drives shuffle, repeat and
//! the per-button enable flags from real state -- souvlaki never calls
//! `SetShuffleEnabled`/`SetAutoRepeatMode` at all and pins every button to
//! enabled in `attach`.

use std::ffi::c_void;
use std::time::Duration;

use windows::core::HSTRING;
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    AutoRepeatModeChangeRequestedEventArgs, MediaPlaybackAutoRepeatMode, MediaPlaybackStatus,
    MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    ShuffleEnabledChangeRequestedEventArgs, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsDisplayUpdater,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

use crate::config::PlayerConfig;
use crate::error::{Error, Result};
use crate::state::{Event, PlaybackState, Repeat, Track};
use crate::platform::{Backend, Emitter};

fn duration_from_secs(seconds: f64) -> TimeSpan {
    if !seconds.is_finite() || seconds <= 0.0 {
        return TimeSpan::default();
    }
    TimeSpan::from(Duration::from_secs_f64(seconds))
}

fn repeat_to_winrt(mode: Repeat) -> MediaPlaybackAutoRepeatMode {
    match mode {
        Repeat::Off => MediaPlaybackAutoRepeatMode::None,
        Repeat::One => MediaPlaybackAutoRepeatMode::Track,
        Repeat::All => MediaPlaybackAutoRepeatMode::List,
    }
}

fn repeat_from_winrt(mode: MediaPlaybackAutoRepeatMode) -> Repeat {
    match mode {
        MediaPlaybackAutoRepeatMode::Track => Repeat::One,
        MediaPlaybackAutoRepeatMode::List => Repeat::All,
        _ => Repeat::Off,
    }
}

pub struct SmtcBackend {
    controls: SystemMediaTransportControls,
    display_updater: SystemMediaTransportControlsDisplayUpdater,
    timeline: SystemMediaTransportControlsTimelineProperties,
    /// windows-rs 0.62 returns event registration tokens as plain `i64`.
    tokens: Vec<(&'static str, i64)>,
    last_track: Option<Track>,
    last_duration: f64,
}

// SMTC objects are WinRT agile objects, so they can be used from whichever
// thread napi hands us. The addon only ever touches them from the Electron main
// thread anyway; events arrive on a WinRT pool thread and are forwarded through
// the threadsafe function rather than touching this struct.
unsafe impl Send for SmtcBackend {}

impl SmtcBackend {
    pub(crate) fn start(config: &PlayerConfig, emit: Emitter) -> Result<Self> {
        let hwnd = config.hwnd.ok_or_else(|| {
            Error::Config("Windows media controls require PlayerConfig::hwnd".to_string())
        })?;

        let interop = windows::core::factory::<
            SystemMediaTransportControls,
            ISystemMediaTransportControlsInterop,
        >()
        .map_err(|error| Error::Unavailable(format!("SMTC interop: {error}")))?;

        let controls: SystemMediaTransportControls =
            unsafe { interop.GetForWindow(HWND(hwnd as usize as *mut c_void)) }
                .map_err(|error| Error::Unavailable(format!("GetForWindow: {error}")))?;

        let display_updater = controls
            .DisplayUpdater()
            .map_err(|error| Error::Unavailable(format!("DisplayUpdater: {error}")))?;
        let timeline = SystemMediaTransportControlsTimelineProperties::new()
            .map_err(|error| Error::Unavailable(format!("timeline properties: {error}")))?;

        display_updater
            .SetType(MediaPlaybackType::Music)
            .map_err(|error| Error::Unavailable(format!("playback type: {error}")))?;

        let mut backend = Self {
            controls,
            display_updater,
            timeline,
            tokens: Vec::new(),
            last_track: None,
            last_duration: -1.0,
        };

        backend.attach(emit)?;
        backend
            .controls
            .SetIsEnabled(true)
            .map_err(|error| Error::Unavailable(format!("enabling SMTC: {error}")))?;

        Ok(backend)
    }

    fn attach(&mut self, emit: Emitter) -> Result<()> {
        let button_emit = emit.clone();
        let button_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };

            let command = match args.Button()? {
                SystemMediaTransportControlsButton::Play => Event::Play,
                SystemMediaTransportControlsButton::Pause => Event::Pause,
                SystemMediaTransportControlsButton::Stop => Event::Stop,
                SystemMediaTransportControlsButton::Next => Event::Next,
                SystemMediaTransportControlsButton::Previous => Event::Previous,
                // SMTC gives no magnitude for these, so we pick the same 10s the
                // Web MediaSession default uses, matching the renderer's
                // seekbackward/seekforward handlers.
                SystemMediaTransportControlsButton::FastForward => Event::SeekBy(10.0),
                SystemMediaTransportControlsButton::Rewind => Event::SeekBy(-10.0),
                _ => return Ok(()),
            };

            button_emit(command);
            Ok(())
        });

        let token = self
            .controls
            .ButtonPressed(&button_handler)
            .map_err(|error| Error::Unavailable(format!("registering ButtonPressed: {error}")))?;
        self.tokens.push(("button", token));

        let position_emit = emit.clone();
        let position_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let requested = Duration::from(args.RequestedPlaybackPosition()?);
            position_emit(Event::SeekTo(requested));
            Ok(())
        });

        let token = self
            .controls
            .PlaybackPositionChangeRequested(&position_handler)
            .map_err(|error| Error::Unavailable(format!("registering PlaybackPositionChangeRequested: {error}")))?;
        self.tokens.push(("position", token));

        let shuffle_emit = emit.clone();
        let shuffle_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            ShuffleEnabledChangeRequestedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            shuffle_emit(Event::SetShuffle(args.RequestedShuffleEnabled()?));
            Ok(())
        });

        let token = self
            .controls
            .ShuffleEnabledChangeRequested(&shuffle_handler)
            .map_err(|error| Error::Unavailable(format!("registering ShuffleEnabledChangeRequested: {error}")))?;
        self.tokens.push(("shuffle", token));

        let repeat_emit = emit;
        let repeat_handler = TypedEventHandler::<
            SystemMediaTransportControls,
            AutoRepeatModeChangeRequestedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            repeat_emit(Event::SetRepeat(repeat_from_winrt(
                args.RequestedAutoRepeatMode()?,
            )));
            Ok(())
        });

        let token = self
            .controls
            .AutoRepeatModeChangeRequested(&repeat_handler)
            .map_err(|error| Error::Unavailable(format!("registering AutoRepeatModeChangeRequested: {error}")))?;
        self.tokens.push(("repeat", token));

        Ok(())
    }

    fn apply_metadata(&mut self, state: &PlaybackState) -> windows::core::Result<()> {
        let properties = self.display_updater.MusicProperties()?;
        let track = state.track.clone().unwrap_or_default();

        properties.SetTitle(&HSTRING::from(if track.title.is_empty() {
            "Orchard"
        } else {
            track.title.as_str()
        }))?;

        let artists = state.artists();
        let primary = artists.first().cloned().unwrap_or_default();
        properties.SetArtist(&HSTRING::from(primary.as_str()))?;
        properties.SetAlbumArtist(&HSTRING::from(primary.as_str()))?;
        properties.SetAlbumTitle(&HSTRING::from(track.album.as_str()))?;

        // Orchard's artwork is always a remote https URL (nowArtworkImage falls
        // back to track.artwork_url, both of which are YouTube CDN links), so
        // there is no local-file branch to carry here.
        if !track.artwork_url.is_empty() {
            let stream = RandomAccessStreamReference::CreateFromUri(&Uri::CreateUri(
                &HSTRING::from(track.artwork_url.as_str()),
            )?)?;
            self.display_updater.SetThumbnail(&stream)?;
        } else {
            self.display_updater.SetThumbnail(None)?;
        }

        self.display_updater.Update()?;
        Ok(())
    }
}

impl Backend for SmtcBackend {
    fn set_state(&mut self, state: &PlaybackState) -> Result<()> {
        let apply = || -> windows::core::Result<()> {
            self.controls.SetPlaybackStatus(if !state.has_track() {
                MediaPlaybackStatus::Stopped
            } else if state.playing {
                MediaPlaybackStatus::Playing
            } else {
                MediaPlaybackStatus::Paused
            })?;

            // Souvlaki hardcodes these to true in attach, so the shell offers a
            // next button on an empty queue and a scrubber on a live stream.
            self.controls.SetIsPlayEnabled(state.has_track())?;
            self.controls.SetIsPauseEnabled(state.has_track())?;
            self.controls.SetIsStopEnabled(state.has_track())?;
            self.controls.SetIsNextEnabled(state.capabilities.can_go_next)?;
            self.controls.SetIsPreviousEnabled(state.capabilities.can_go_previous)?;
            self.controls.SetIsFastForwardEnabled(state.capabilities.can_seek)?;
            self.controls.SetIsRewindEnabled(state.capabilities.can_seek)?;

            self.controls.SetShuffleEnabled(state.shuffle)?;
            self.controls
                .SetAutoRepeatMode(repeat_to_winrt(state.repeat))?;

            let duration = duration_from_secs(state.duration_secs());
            self.timeline.SetStartTime(TimeSpan::default())?;
            self.timeline.SetMinSeekTime(TimeSpan::default())?;
            self.timeline.SetEndTime(duration)?;
            self.timeline.SetMaxSeekTime(duration)?;
            self.timeline
                .SetPosition(duration_from_secs(state.position_secs()))?;
            self.controls.UpdateTimelineProperties(&self.timeline)?;

            Ok(())
        };

        apply().map_err(|error| Error::Publish(format!("SMTC state: {error}")))?;

        // The display updater rebuilds a thumbnail stream, so only touch it when
        // the track actually changed rather than on every timeupdate.
        let track_changed = self.last_track != state.track
            || (self.last_duration - state.duration_secs()).abs() > f64::EPSILON;

        if track_changed {
            self.apply_metadata(state)
                .map_err(|error| Error::Publish(format!("SMTC metadata: {error}")))?;
            self.last_track = state.track.clone();
            self.last_duration = state.duration_secs();
        }

        Ok(())
    }

    fn stop(&mut self) {
        for (kind, token) in self.tokens.drain(..) {
            let _ = match kind {
                "button" => self.controls.RemoveButtonPressed(token),
                "position" => self.controls.RemovePlaybackPositionChangeRequested(token),
                "shuffle" => self.controls.RemoveShuffleEnabledChangeRequested(token),
                "repeat" => self.controls.RemoveAutoRepeatModeChangeRequested(token),
                _ => Ok(()),
            };
        }

        let _ = self.controls.SetIsEnabled(false);
    }
}
