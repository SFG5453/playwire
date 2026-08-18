// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! The playback state you publish and the events the OS sends back.

use std::time::Duration;

/// What is currently playing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// Stable identifier for this track. Used to build the MPRIS
    /// `mpris:trackid` object path, and to detect track changes so artwork and
    /// metadata are only republished when they actually change.
    ///
    /// Any string is accepted; characters outside the D-Bus object-path
    /// alphabet are substituted.
    pub id: String,
    pub title: String,
    /// Every credited artist. The first is treated as the primary artist on
    /// backends that only accept one.
    pub artists: Vec<String>,
    pub album: String,
    /// Cover art. On Linux and Windows this is handed to the OS as a URL, so it
    /// should be `http(s)://` or `file://`. On macOS it is fetched and decoded
    /// into an `NSImage` off-thread.
    pub artwork_url: String,
    /// A link to this track in your service, exposed as MPRIS `xesam:url`.
    pub url: String,
}

impl Track {
    /// The primary artist, or an empty string when none is credited.
    pub fn primary_artist(&self) -> &str {
        self.artists.first().map(String::as_str).unwrap_or("")
    }
}

/// Repeat mode, in the vocabulary every backend agrees on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
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
/// means `can_seek: false`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub can_go_next: bool,
    pub can_go_previous: bool,
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

/// A complete snapshot of the player. Publish this whenever anything changes;
/// backends diff against the previous snapshot and only touch what moved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    /// `None` means stopped with nothing loaded.
    pub track: Option<Track>,
    pub playing: bool,
    pub position: Duration,
    /// `None` for live streams and anything else without a known length.
    pub duration: Option<Duration>,
    /// 0.0 to 1.0. Only MPRIS reports volume changes back.
    pub volume: f64,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub capabilities: Capabilities,
}

impl PlaybackState {
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
    #[cfg_attr(not(all(unix, not(any(target_os = "macos", target_os = "ios")))), allow(dead_code))]
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
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Play,
    Pause,
    /// Play if paused, pause if playing.
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Seek to an absolute position.
    SeekTo(Duration),
    /// Seek forwards (positive) or backwards (negative) by this many seconds.
    SeekBy(f64),
    /// Requested volume, 0.0 to 1.0. MPRIS only.
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(Repeat),
    /// A URI the player was asked to open. MPRIS only.
    OpenUri(String),
    /// Bring the player's window to the front. MPRIS only.
    Raise,
    /// The player was asked to exit. MPRIS only.
    Quit,
}
