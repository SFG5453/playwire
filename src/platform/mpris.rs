// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! MPRIS v2 over D-Bus.
//!
//! Written against zbus 5 rather than adapted from souvlaki, whose MPRIS
//! backend hardcodes every `Can*` property to true, emits `mpris:trackid` as the
//! literal `/`, exposes neither `Shuffle` nor `LoopStatus`, omits `DesktopEntry`
//! entirely, and misspells `HasTrackList`.

use std::collections::HashMap;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use zbus::blocking::Connection;
use zbus::object_server::SignalEmitter;
use zbus::interface;
use zvariant::{ObjectPath, OwnedValue, Value};

use crate::config::PlayerConfig;
use crate::error::{Error, Result};
use crate::platform::{Backend, Emitter};
use crate::state::{Event, PlaybackState, Repeat};

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK_PATH: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

fn loop_status_to_repeat(value: &str) -> Repeat {
    match value {
        "Track" => Repeat::One,
        "Playlist" => Repeat::All,
        _ => Repeat::Off,
    }
}

fn repeat_to_loop_status(mode: Repeat) -> &'static str {
    match mode {
        Repeat::Off => "None",
        Repeat::One => "Track",
        Repeat::All => "Playlist",
    }
}

/// MPRIS track ids are object paths, so every character outside the D-Bus path
/// alphabet is substituted. Clients key their metadata cache on this, so a
/// player that reuses one id for every track has its art and title cached
/// against the first track forever.
fn track_object_path(state: &PlaybackState, prefix: &str) -> ObjectPath<'static> {
    let Some(track) = &state.track else {
        return ObjectPath::try_from(NO_TRACK_PATH).unwrap();
    };

    if track.id.is_empty() {
        return ObjectPath::try_from(NO_TRACK_PATH).unwrap();
    }

    let sanitized: String = track
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();

    ObjectPath::try_from(format!("{prefix}/{sanitized}"))
        .unwrap_or_else(|_| ObjectPath::try_from(NO_TRACK_PATH).unwrap())
}

fn metadata_map(state: &PlaybackState, prefix: &str) -> HashMap<String, OwnedValue> {
    let mut metadata = HashMap::new();
    let mut insert = |key: &str, value: Value<'_>| {
        if let Ok(owned) = OwnedValue::try_from(value) {
            metadata.insert(key.to_string(), owned);
        }
    };

    insert("mpris:trackid", Value::from(track_object_path(state, prefix)));

    let Some(track) = &state.track else {
        return metadata;
    };

    insert("xesam:title", Value::from(track.title.clone()));
    insert("xesam:album", Value::from(track.album.clone()));
    insert("xesam:artist", Value::from(state.artists()));

    if !track.artwork_url.is_empty() {
        insert("mpris:artUrl", Value::from(track.artwork_url.clone()));
    }

    if state.duration_secs() > 0.0 {
        let micros = (state.duration_secs() * 1_000_000.0).round() as i64;
        insert("mpris:length", Value::from(micros));
    }

    if !track.url.is_empty() {
        insert("xesam:url", Value::from(track.url.clone()));
    }

    metadata
}

struct Shared {
    state: PlaybackState,
    emit: Emitter,
    track_id_prefix: String,
}

impl Shared {
    fn send(&self, command: Event) {
        (self.emit)(command);
    }
}

struct RootInterface {
    shared: Arc<Mutex<Shared>>,
    identity: String,
    desktop_entry: String,
    supported_uri_schemes: Vec<String>,
    supported_mime_types: Vec<String>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl RootInterface {
    fn raise(&self) {
        self.shared.lock().unwrap().send(Event::Raise);
    }

    fn quit(&self) {
        self.shared.lock().unwrap().send(Event::Quit);
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn fullscreen(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn set_fullscreen(&self, _value: bool) {}

    #[zbus(property)]
    fn can_set_fullscreen(&self) -> bool {
        false
    }

    /// Spelled per the MPRIS specification. Souvlaki exports this as
    /// `HasTracklist`, which spec-conforming clients simply do not read.
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    /// The property GNOME and KDE use to resolve the player to its .desktop
    /// file, and therefore to show an app icon in the media widget. Souvlaki
    /// does not export it at all.
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        self.desktop_entry.clone()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        self.supported_uri_schemes.clone()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        self.supported_mime_types.clone()
    }
}

struct PlayerInterface {
    shared: Arc<Mutex<Shared>>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    fn next(&self) {
        self.shared.lock().unwrap().send(Event::Next);
    }

    fn previous(&self) {
        self.shared.lock().unwrap().send(Event::Previous);
    }

    fn pause(&self) {
        self.shared.lock().unwrap().send(Event::Pause);
    }

    fn play_pause(&self) {
        self.shared.lock().unwrap().send(Event::PlayPause);
    }

    fn stop(&self) {
        self.shared.lock().unwrap().send(Event::Stop);
    }

    fn play(&self) {
        self.shared.lock().unwrap().send(Event::Play);
    }

    fn seek(&self, offset: i64) {
        self.shared
            .lock()
            .unwrap()
            .send(Event::SeekBy(offset as f64 / 1_000_000.0));
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let shared = self.shared.lock().unwrap();

        // Per spec, a SetPosition naming a track that is no longer current must
        // be ignored -- otherwise a click landing just as the track changes
        // seeks the new track to the old one's offset.
        if track_id != track_object_path(&shared.state, &shared.track_id_prefix) {
            return;
        }

        if position < 0 {
            return;
        }

        let seconds = position as f64 / 1_000_000.0;
        if shared.state.duration_secs() > 0.0 && seconds > shared.state.duration_secs() {
            return;
        }

        shared.send(Event::SeekTo(Duration::from_secs_f64(seconds)));
    }

    fn open_uri(&self, _uri: String) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let shared = self.shared.lock().unwrap();

        if !shared.state.has_track() {
            return "Stopped".to_string();
        }

        if shared.state.playing {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        }
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        let shared = self.shared.lock().unwrap();
        repeat_to_loop_status(shared.state.repeat).to_string()
    }

    #[zbus(property)]
    fn set_loop_status(&self, value: String) {
        let mut shared = self.shared.lock().unwrap();
        let mode = loop_status_to_repeat(&value);
        shared.state.repeat = mode;
        shared.send(Event::SetRepeat(mode));
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.shared.lock().unwrap().state.shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&self, value: bool) {
        let mut shared = self.shared.lock().unwrap();
        shared.state.shuffle = value;
        shared.send(Event::SetShuffle(value));
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.shared.lock().unwrap().state.volume_clamped()
    }

    #[zbus(property)]
    fn set_volume(&self, value: f64) {
        let mut shared = self.shared.lock().unwrap();
        let clamped = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            return;
        };
        shared.state.volume = clamped;
        shared.send(Event::SetVolume(clamped));
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let shared = self.shared.lock().unwrap();
        metadata_map(&shared.state, &shared.track_id_prefix)
    }

    /// Deliberately not `emits_changed`: the spec has clients extrapolate
    /// position from the last `Seeked` signal plus `Rate`, and a property
    /// change on every timeupdate would be a D-Bus message per frame.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        let shared = self.shared.lock().unwrap();
        (shared.state.position_secs() * 1_000_000.0).round() as i64
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_rate(&self, _value: f64) {}

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.shared.lock().unwrap().state.capabilities.can_go_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.shared.lock().unwrap().state.capabilities.can_go_previous
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.shared.lock().unwrap().state.has_track()
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.shared.lock().unwrap().state.has_track()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.shared.lock().unwrap().state.capabilities.can_seek
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub struct MprisBackend {
    connection: Connection,
    shared: Arc<Mutex<Shared>>,
    last: Option<PlaybackState>,
}

impl MprisBackend {
    pub(crate) fn start(config: &PlayerConfig, emit: Emitter) -> Result<Self> {
        let shared = Arc::new(Mutex::new(Shared {
            state: PlaybackState::default(),
            emit,
            track_id_prefix: config.track_id_prefix.trim_end_matches('/').to_string(),
        }));

        let root = RootInterface {
            shared: shared.clone(),
            identity: config.identity.clone(),
            desktop_entry: config.desktop_entry.clone(),
            supported_uri_schemes: config.supported_uri_schemes.clone(),
            supported_mime_types: config.supported_mime_types.clone(),
        };
        let player = PlayerInterface {
            shared: shared.clone(),
        };

        // Serve the fully-populated object before requesting the well-known
        // name. Clients such as Plasma read the player the moment
        // NameOwnerChanged fires, so claiming the name first leaves a window
        // where they can cache an empty player and never re-read it.
        let connection = Connection::session()
            .map_err(|error| Error::Unavailable(format!("session bus: {error}")))?;

        connection
            .object_server()
            .at(OBJECT_PATH, root)
            .map_err(|error| Error::Unavailable(format!("serving MediaPlayer2: {error}")))?;
        connection
            .object_server()
            .at(OBJECT_PATH, player)
            .map_err(|error| Error::Unavailable(format!("serving MediaPlayer2.Player: {error}")))?;

        let name = format!("org.mpris.MediaPlayer2.{}", config.bus_name);
        connection
            .request_name(name.as_str())
            .map_err(|error| Error::Unavailable(format!("claiming {name}: {error}")))?;

        Ok(Self {
            connection,
            shared,
            last: None,
        })
    }

    fn player_ref(&self) -> Result<zbus::blocking::object_server::InterfaceRef<PlayerInterface>> {
        self.connection
            .object_server()
            .interface::<_, PlayerInterface>(OBJECT_PATH)
            .map_err(|error| Error::Publish(format!("player interface missing: {error}")))
    }
}

impl Backend for MprisBackend {
    fn set_state(&mut self, state: &PlaybackState) -> Result<()> {
        {
            let mut shared = self.shared.lock().unwrap();
            shared.state = state.clone();
        }

        let player = self.player_ref()?;
        let emitter = player.signal_emitter();
        let previous = self.last.take();

        // Only emit for properties that actually changed. Blanket-emitting on
        // every timeupdate makes Plasma's widget flicker and wakes every MPRIS
        // client on the bus several times a second.
        let changed_track = previous
            .as_ref()
            .map(|old| {
                old.track != state.track || old.duration_secs() != state.duration_secs()
            })
            .unwrap_or(true);

        let emit_property = |changed: bool, emit: &dyn Fn() -> zbus::Result<()>| {
            if changed {
                let _ = emit();
            }
        };

        let interface = player.get();

        emit_property(changed_track, &|| {
            futures_lite_block(interface.metadata_changed(emitter))
        });

        let playback_changed = previous
            .as_ref()
            .map(|old| old.playing != state.playing || old.has_track() != state.has_track())
            .unwrap_or(true);
        emit_property(playback_changed, &|| {
            futures_lite_block(interface.playback_status_changed(emitter))
        });

        let repeat_changed = previous
            .as_ref()
            .map(|old| old.repeat != state.repeat)
            .unwrap_or(true);
        emit_property(repeat_changed, &|| {
            futures_lite_block(interface.loop_status_changed(emitter))
        });

        let shuffle_changed = previous
            .as_ref()
            .map(|old| old.shuffle != state.shuffle)
            .unwrap_or(true);
        emit_property(shuffle_changed, &|| {
            futures_lite_block(interface.shuffle_changed(emitter))
        });

        let volume_changed = previous
            .as_ref()
            .map(|old| old.volume_clamped() != state.volume_clamped())
            .unwrap_or(true);
        emit_property(volume_changed, &|| {
            futures_lite_block(interface.volume_changed(emitter))
        });

        let capability_changed = previous
            .as_ref()
            .map(|old| {
                old.capabilities.can_go_next != state.capabilities.can_go_next
                    || old.capabilities.can_go_previous != state.capabilities.can_go_previous
                    || old.capabilities.can_seek != state.capabilities.can_seek
                    || old.has_track() != state.has_track()
            })
            .unwrap_or(true);
        emit_property(capability_changed, &|| {
            futures_lite_block(interface.can_go_next_changed(emitter))?;
            futures_lite_block(interface.can_go_previous_changed(emitter))?;
            futures_lite_block(interface.can_seek_changed(emitter))?;
            futures_lite_block(interface.can_play_changed(emitter))?;
            futures_lite_block(interface.can_pause_changed(emitter))
        });

        // A discontinuity the client cannot extrapolate: either an explicit seek
        // or a new track resetting to zero. This is what lets a client's scrubber
        // stay accurate without us emitting Position on every frame.
        let seeked = previous
            .as_ref()
            .map(|old| {
                let drift = (state.position_secs() - old.position_secs()).abs();
                changed_track || drift > 1.5
            })
            .unwrap_or(false);

        if seeked {
            let micros = (state.position_secs() * 1_000_000.0).round() as i64;
            let _ = futures_lite_block(PlayerInterface::seeked(emitter, micros));
        }

        self.last = Some(state.clone());
        Ok(())
    }

    fn stop(&mut self) {
        let _ = self
            .connection
            .object_server()
            .remove::<PlayerInterface, _>(OBJECT_PATH);
        let _ = self
            .connection
            .object_server()
            .remove::<RootInterface, _>(OBJECT_PATH);
    }
}

/// zbus' property-changed helpers are async even on the blocking connection.
/// They complete without ever yielding to a reactor, so a minimal block-on is
/// enough and avoids pulling in a full async runtime.
fn futures_lite_block<T>(future: impl std::future::Future<Output = T>) -> T {
    zbus::block_on(future)
}
