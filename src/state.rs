// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! The playback state you publish and the events the OS sends back.

use std::time::Duration;

/// What is currently playing.
///
/// Only [`id`](Track::id) carries behaviour; the rest is display metadata that
/// each backend maps onto its platform's fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// Stable identifier for this track.
    ///
    /// Used to build the MPRIS `mpris:trackid` object path, and to detect track
    /// changes so artwork and metadata are only republished when they actually
    /// change. Any string is accepted; characters outside the D-Bus object-path
    /// alphabet are substituted.
    ///
    /// Reusing one id across different tracks makes MPRIS clients cache the
    /// first track's metadata against every subsequent one.
    pub id: String,
    /// Track title. Shown as the primary line in every OS media widget.
    pub title: String,
    /// Every credited artist.
    ///
    /// MPRIS receives the full list as `xesam:artist`. Windows and macOS accept
    /// only one, so they use the first; see [`Track::primary_artist`].
    pub artists: Vec<String>,
    /// Album title. Empty is fine and simply omits the field.
    pub album: String,
    /// Cover art.
    ///
    /// On Linux and Windows this is handed to the OS as a URL, so it should be
    /// `http(s)://` or `file://`. On macOS it is fetched and decoded into an
    /// `NSImage` on a background thread, and applied only if the track has not
    /// changed by the time it finishes.
    ///
    /// Empty clears any previously published artwork.
    pub artwork_url: String,
    /// A link to this track in your service, published as MPRIS `xesam:url`.
    ///
    /// Ignored on Windows and macOS, which have no equivalent field.
    pub url: String,
}

impl Track {
    /// The primary artist, or an empty string when none is credited.
    ///
    /// This is what the Windows and macOS backends display, since neither
    /// accepts an artist list.
    pub fn primary_artist(&self) -> &str {
        self.artists.first().map(String::as_str).unwrap_or("")
    }
}

/// Repeat mode, in the vocabulary every backend agrees on.
///
/// Maps to MPRIS `LoopStatus` (`None`/`Track`/`Playlist`), SMTC
/// `MediaPlaybackAutoRepeatMode`, and macOS `MPRepeatType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    /// Play through and stop.
    #[default]
    Off,
    /// Repeat the current track.
    One,
    /// Repeat the whole queue or playlist.
    All,
}

/// What the player can currently do.
///
/// These reach the OS, which greys out controls accordingly, so they should
/// track reality: an empty queue means `can_go_next: false`, and a live stream
/// means `can_seek: false`. [`Default`] enables everything, which is the right
/// starting point for a player that always has a queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether a next track exists. Controls the OS next button.
    pub can_go_next: bool,
    /// Whether a previous track exists. Controls the OS previous button.
    pub can_go_previous: bool,
    /// Whether seeking is possible. False for live streams, which hides or
    /// disables the scrubber.
    pub can_seek: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            can_go_next: true,
            can_go_previous: true,
            can_seek: true,
        }
    }
}

/// A complete snapshot of the player.
///
/// Publish this with [`MediaControls::set_state`](crate::MediaControls::set_state)
/// whenever anything changes, including on every position tick. Backends diff
/// against the previous snapshot and only touch what actually moved, so
/// republishing unchanged state is cheap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    /// The current track, or `None` when stopped with nothing loaded.
    ///
    /// `None` reports `Stopped` to the OS and clears the widget.
    pub track: Option<Track>,
    /// Whether playback is currently advancing.
    ///
    /// Drives `Playing` versus `Paused`, and on macOS the playback rate that
    /// lets Control Center's scrubber animate between updates.
    pub playing: bool,
    /// How far into the track playback has reached.
    ///
    /// Clamped to [`duration`](PlaybackState::duration) internally, so a stale
    /// value left over from a previous track cannot push a client's scrubber
    /// past the end.
    pub position: Duration,
    /// Track length, or `None` for live streams and anything else without a
    /// known length.
    ///
    /// `None` omits the length entirely rather than publishing zero, which is
    /// what stops clients drawing a full-width or jumping progress bar.
    pub duration: Option<Duration>,
    /// Playback volume from 0.0 to 1.0. Values outside that range are clamped.
    ///
    /// Only MPRIS publishes volume or reports [`Event::SetVolume`] back; the
    /// field is ignored on Windows and macOS.
    pub volume: f64,
    /// Current repeat mode.
    pub repeat: Repeat,
    /// Whether shuffle is enabled.
    pub shuffle: bool,
    /// Which controls the OS should offer right now.
    pub capabilities: Capabilities,
}

impl PlaybackState {
    /// Whether a track is loaded.
    ///
    /// Backends use this to decide between `Paused` and `Stopped`, and to
    /// enable or disable the play and pause controls.
    pub fn has_track(&self) -> bool {
        self.track.is_some()
    }

    pub(crate) fn artists(&self) -> Vec<String> {
        self.track
            .as_ref()
            .map(|track| track.artists.clone())
            .unwrap_or_default()
    }

    pub(crate) fn duration_secs(&self) -> f64 {
        self.duration.map(|value| value.as_secs_f64()).unwrap_or(0.0)
    }

    /// Clamped to the track length: a stale position left over from the previous
    /// track would otherwise make a client's scrubber jump past the end.
    pub(crate) fn position_secs(&self) -> f64 {
        let position = self.position.as_secs_f64();
        let duration = self.duration_secs();

        if duration > 0.0 {
            position.min(duration)
        } else {
            position
        }
    }

    /// Only MPRIS exposes volume, so this is dead code on other targets.
    #[cfg_attr(
        not(all(unix, not(any(target_os = "macos", target_os = "ios")))),
        allow(dead_code)
    )]
    pub(crate) fn volume_clamped(&self) -> f64 {
        if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// A control the user activated, from a media key, the OS shell, or another
/// MPRIS client.
///
/// Delivered to the handler passed to
/// [`MediaControls::new`](crate::MediaControls::new). Which variants can arrive
/// depends on the platform; see the feature table in the [crate docs](crate).
///
/// This enum is `#[non_exhaustive]`, so match with a catch-all arm.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Begin or resume playback.
    Play,
    /// Pause, keeping the current position.
    Pause,
    /// Play if paused, pause if playing.
    ///
    /// Emitted by the play/pause media key and by MPRIS `PlayPause`. Windows
    /// and macOS resolve the key to an explicit [`Play`](Event::Play) or
    /// [`Pause`](Event::Pause) instead.
    PlayPause,
    /// Stop playback and return to the start.
    Stop,
    /// Skip to the next track.
    Next,
    /// Skip to the previous track.
    Previous,
    /// Seek to an absolute position.
    ///
    /// Already validated against the published duration, and against the
    /// current track id on MPRIS, so a request arriving just as the track
    /// changes is dropped rather than applied to the wrong track.
    SeekTo(Duration),
    /// Seek forwards (positive) or backwards (negative) by this many seconds.
    ///
    /// MPRIS supplies the offset. Windows fast-forward and rewind carry no
    /// magnitude, so they arrive as ±10 seconds.
    SeekBy(f64),
    /// Requested volume, 0.0 to 1.0.
    ///
    /// MPRIS only. Apply it and publish the new volume back; the OS does not
    /// assume the request succeeded.
    SetVolume(f64),
    /// Requested shuffle state.
    SetShuffle(bool),
    /// Requested repeat mode.
    SetRepeat(Repeat),
    /// A URI the player was asked to open. MPRIS only.
    OpenUri(String),
    /// Bring the player's window to the front. MPRIS only.
    Raise,
    /// The player was asked to exit. MPRIS only.
    Quit,
}
