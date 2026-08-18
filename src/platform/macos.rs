// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! Now Playing / Control Center.
//!
//! Uses objc2's typed MediaPlayer bindings rather than souvlaki's hand-rolled
//! `msg_send!` calls -- notably souvlaki reads the seek position out of the
//! private `_positionTime` ivar, where the framework exposes a public
//! `positionTime` accessor. It also never sets
//! `MPNowPlayingInfoPropertyPlaybackRate`, which is why Control Center's
//! scrubber sits frozen even while elapsed time updates.

use std::ptr::NonNull;
use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};


use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_app_kit::NSImage;
use objc2_foundation::{NSCopying, NSMutableDictionary, NSNumber, NSSize, NSString, NSURL};
use objc2_media_player::{
    MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommandCenter,
    MPRemoteCommandEvent, MPRemoteCommandHandlerStatus, MPRepeatType, MPShuffleType,
};

use crate::config::PlayerConfig;
use crate::error::Result;
use crate::state::{Event, PlaybackState, Repeat, Track};
use crate::platform::{Backend, Emitter};

/// Artwork loads off the main thread, so a slow fetch for a track the user has
/// already skipped past must not overwrite the current track's art. Each
/// metadata publish takes a ticket; a load only applies if it still holds the
/// latest one.
static ARTWORK_GENERATION: AtomicUsize = AtomicUsize::new(0);

pub struct NowPlayingBackend {
    /// Opaque target objects returned by `addTargetWithHandler`, kept so the
    /// handlers can be removed again on teardown.
    targets: Vec<(&'static str, Retained<AnyObject>)>,
    last_track: Option<Track>,
    last_duration: f64,
}

// The Objective-C objects here are only touched from the Electron main thread.
// Commands arrive on the main run loop and are forwarded through the threadsafe
// function rather than mutating this struct.
unsafe impl Send for NowPlayingBackend {}

fn ns_string(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

impl NowPlayingBackend {
    pub(crate) fn start(_config: &PlayerConfig, emit: Emitter) -> Result<Self> {
        let mut backend = Self {
            targets: Vec::new(),
            last_track: None,
            last_duration: -1.0,
        };

        backend.attach(emit);
        Ok(backend)
    }

    fn attach(&mut self, emit: Emitter) {
        let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };

        // Commands carrying no payload all share this shape.
        let mut simple = |name: &'static str,
                          command: Retained<objc2_media_player::MPRemoteCommand>,
                          make: fn() -> Event| {
            let emit = emit.clone();
            let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
                emit(make());
                MPRemoteCommandHandlerStatus::Success
            });
            unsafe {
                command.setEnabled(true);
                let target = command.addTargetWithHandler(&handler);
                self.targets.push((name, target));
            }
        };

        unsafe {
            simple("play", center.playCommand(), || Event::Play);
            simple("pause", center.pauseCommand(), || Event::Pause);
            simple("toggle", center.togglePlayPauseCommand(), || {
                Event::PlayPause
            });
            simple("next", center.nextTrackCommand(), || Event::Next);
            simple("previous", center.previousTrackCommand(), || {
                Event::Previous
            });
            // souvlaki never wires stopCommand at all.
            simple("stop", center.stopCommand(), || Event::Stop);
        }

        let position_emit = emit.clone();
        let position_handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            // The public accessor, rather than souvlaki's private `_positionTime`
            // ivar read.
            let seconds = unsafe {
                let event = event.as_ref();
                let event: &objc2_media_player::MPChangePlaybackPositionCommandEvent =
                    &*(event as *const MPRemoteCommandEvent as *const _);
                event.positionTime()
            };
            position_emit(Event::SeekTo(Duration::from_secs_f64(seconds)));
            MPRemoteCommandHandlerStatus::Success
        });
        unsafe {
            let command = center.changePlaybackPositionCommand();
            command.setEnabled(true);
            let target = command.addTargetWithHandler(&position_handler);
            self.targets.push(("position", target));
        }

        let shuffle_emit = emit.clone();
        let shuffle_handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            let shuffle = unsafe {
                let event = event.as_ref();
                let event: &objc2_media_player::MPChangeShuffleModeCommandEvent =
                    &*(event as *const MPRemoteCommandEvent as *const _);
                event.shuffleType() != MPShuffleType::Off
            };
            shuffle_emit(Event::SetShuffle(shuffle));
            MPRemoteCommandHandlerStatus::Success
        });
        unsafe {
            let command = center.changeShuffleModeCommand();
            command.setEnabled(true);
            let target = command.addTargetWithHandler(&shuffle_handler);
            self.targets.push(("shuffle", target));
        }

        let repeat_emit = emit;
        let repeat_handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            let mode = unsafe {
                let event = event.as_ref();
                let event: &objc2_media_player::MPChangeRepeatModeCommandEvent =
                    &*(event as *const MPRemoteCommandEvent as *const _);
                match event.repeatType() {
                    MPRepeatType::One => Repeat::One,
                    MPRepeatType::All => Repeat::All,
                    _ => Repeat::Off,
                }
            };
            repeat_emit(Event::SetRepeat(mode));
            MPRemoteCommandHandlerStatus::Success
        });
        unsafe {
            let command = center.changeRepeatModeCommand();
            command.setEnabled(true);
            let target = command.addTargetWithHandler(&repeat_handler);
            self.targets.push(("repeat", target));
        }
    }

    fn publish_metadata(&mut self, state: &PlaybackState) {
        let generation = ARTWORK_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
        let info = NSMutableDictionary::<NSString, AnyObject>::new();

        let track = state.track.clone().unwrap_or_default();
        let artists = state.artists();

        unsafe {
            info.setObject_forKey(
                &*ns_string(&track.title) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPMediaItemPropertyTitle),
            );
            info.setObject_forKey(
                &*ns_string(&artists.join(", ")) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPMediaItemPropertyArtist),
            );
            info.setObject_forKey(
                &*ns_string(&track.album) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPMediaItemPropertyAlbumTitle),
            );
            info.setObject_forKey(
                &*NSNumber::new_f64(state.duration_secs()) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPMediaItemPropertyPlaybackDuration),
            );

            center.setNowPlayingInfo(Some(&info.copy()));
        }

        if !track.artwork_url.is_empty() {
            load_artwork_async(track.artwork_url.clone(), generation);
        }
    }

    fn publish_playback(&self, state: &PlaybackState) {
        let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };

        unsafe {
            center.setPlaybackState(if !state.has_track() {
                MPNowPlayingPlaybackState::Stopped
            } else if state.playing {
                MPNowPlayingPlaybackState::Playing
            } else {
                MPNowPlayingPlaybackState::Paused
            });

            let Some(existing) = center.nowPlayingInfo() else {
                return;
            };
            let info = NSMutableDictionary::<NSString, AnyObject>::new();
            info.addEntriesFromDictionary(&existing);

            info.setObject_forKey(
                &*NSNumber::new_f64(state.position_secs()) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(
                    &*MPNowPlayingInfoPropertyElapsedPlaybackTime,
                ),
            );
            // Without a rate, Control Center treats elapsed time as static and
            // the scrubber never advances between updates. souvlaki omits this.
            info.setObject_forKey(
                &*NSNumber::new_f64(if state.playing { 1.0 } else { 0.0 }) as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPNowPlayingInfoPropertyPlaybackRate),
            );

            center.setNowPlayingInfo(Some(&info.copy()));
        }
    }
}

fn load_artwork_async(url: String, generation: usize) {
    std::thread::spawn(move || {
        let Some(image) = NSURL::URLWithString(&ns_string(&url))
            .and_then(|url| NSImage::initWithContentsOfURL(NSImage::alloc(), &url))
        else {
            return;
        };

        // A newer track was published while this download was in flight.
        if ARTWORK_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }

        unsafe {
            let size = image.size();
            // The block owns the Retained for its whole life, so a pointer to it
            // stays valid; deriving one from a temporary clone would dangle as
            // soon as the request handler returned.
            let handler = RcBlock::new(move |_size: NSSize| NonNull::from(&*image));
            let artwork =
                MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                    MPMediaItemArtwork::alloc(),
                    size,
                    &handler,
                );

            let center = MPNowPlayingInfoCenter::defaultCenter();
            let Some(existing) = center.nowPlayingInfo() else {
                return;
            };
            let info = NSMutableDictionary::<NSString, AnyObject>::new();
            info.addEntriesFromDictionary(&existing);
            info.setObject_forKey(
                &*artwork as &AnyObject,
                objc2::runtime::ProtocolObject::from_ref(&*MPMediaItemPropertyArtwork),
            );
            center.setNowPlayingInfo(Some(&info.copy()));
        }
    });
}

impl Backend for NowPlayingBackend {
    fn set_state(&mut self, state: &PlaybackState) -> Result<()> {
        let track_changed = self.last_track != state.track
            || (self.last_duration - state.duration_secs()).abs() > f64::EPSILON;

        if track_changed {
            self.publish_metadata(state);
            self.last_track = state.track.clone();
            self.last_duration = state.duration_secs();
        }

        self.publish_playback(state);
        Ok(())
    }

    fn stop(&mut self) {
        let center = unsafe { MPRemoteCommandCenter::sharedCommandCenter() };

        unsafe {
            // The change* commands are distinct subclasses rather than plain
            // MPRemoteCommand, so each arm has to detach against its own type.
            macro_rules! detach {
                ($command:expr, $target:expr) => {{
                    let command = $command;
                    command.removeTarget(Some(&$target));
                    command.setEnabled(false);
                }};
            }

            for (kind, target) in self.targets.drain(..) {
                match kind {
                    "play" => detach!(center.playCommand(), target),
                    "pause" => detach!(center.pauseCommand(), target),
                    "toggle" => detach!(center.togglePlayPauseCommand(), target),
                    "next" => detach!(center.nextTrackCommand(), target),
                    "previous" => detach!(center.previousTrackCommand(), target),
                    "stop" => detach!(center.stopCommand(), target),
                    "position" => detach!(center.changePlaybackPositionCommand(), target),
                    "shuffle" => detach!(center.changeShuffleModeCommand(), target),
                    "repeat" => detach!(center.changeRepeatModeCommand(), target),
                    _ => continue,
                }
            }

            let center = MPNowPlayingInfoCenter::defaultCenter();
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            center.setNowPlayingInfo(None);
        }
    }
}
