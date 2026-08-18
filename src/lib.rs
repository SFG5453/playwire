// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

#![doc = include_str!("../README.md")]
#![warn(missing_docs, missing_debug_implementations, rustdoc::broken_intra_doc_links)]

mod config;
mod error;
mod platform;
mod state;

pub use config::PlayerConfig;
pub use error::{Error, Result};
pub use state::{Capabilities, Event, PlaybackState, Repeat, Track};

use std::fmt;
use std::sync::Arc;

/// A handle to the OS media controls.
///
/// Created with [`MediaControls::new`], fed with [`MediaControls::set_state`],
/// and detached on drop. Use one per process: on Linux a second instance would
/// try to claim a D-Bus name that is already owned, and on Windows and macOS
/// the underlying services are per-process singletons that a second instance
/// would take over.
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use playwire::{MediaControls, PlaybackState, PlayerConfig, Track};
///
/// # fn main() -> playwire::Result<()> {
/// let mut controls = MediaControls::new(
///     PlayerConfig::new("Fauxplayer"),
///     |event| println!("{event:?}"),
/// )?;
///
/// controls.set_state(&PlaybackState {
///     track: Some(Track { title: "Alison".into(), ..Track::default() }),
///     playing: true,
///     position: Duration::from_secs(12),
///     duration: Some(Duration::from_secs(231)),
///     ..PlaybackState::default()
/// })?;
/// # Ok(())
/// # }
/// ```
pub struct MediaControls {
    backend: Option<Box<dyn platform::Backend>>,
}

impl MediaControls {
    /// Registers with the platform's media service and begins listening.
    ///
    /// `handler` is called from whichever thread the OS delivers controls on --
    /// a D-Bus worker on Linux, a WinRT pool thread on Windows, the main run
    /// loop on macOS -- so it must be `Send + Sync`. Do not block in it, and do
    /// not call back into [`set_state`](Self::set_state) from it; send the
    /// event to your player's own loop instead.
    ///
    /// On Windows, [`PlayerConfig::hwnd`] must be set before calling this.
    ///
    /// # Errors
    ///
    /// - [`Error::Config`] if required platform configuration is missing, such
    ///   as `hwnd` on Windows.
    /// - [`Error::Unavailable`] if the platform's media service cannot be
    ///   reached: no session bus on Linux, WinRT unavailable on Windows, or the
    ///   D-Bus name already claimed.
    /// - [`Error::Unsupported`] on platforms with no integration.
    ///
    /// A player should generally treat all of these as "run without media
    /// controls" rather than as fatal.
    pub fn new(
        config: PlayerConfig,
        handler: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self> {
        let emitter: platform::Emitter = Arc::new(handler);
        let backend = platform::start(&config, emitter)?;

        Ok(Self {
            backend: Some(backend),
        })
    }

    /// Publishes a new snapshot.
    ///
    /// Call this whenever anything changes, including on every position update.
    /// Backends diff against the previous snapshot, so republishing unchanged
    /// state is cheap and will not spam the bus or flicker the OS widget. There
    /// is no separate metadata call: artwork and track details are republished
    /// only when the track or its duration actually changes.
    ///
    /// After [`detach`](Self::detach) this is a no-op returning `Ok`.
    ///
    /// # Errors
    ///
    /// [`Error::Publish`] if the platform rejected the update. This is usually
    /// transient -- a D-Bus hiccup, a window being torn down -- and the next
    /// call will typically succeed, so it is not a reason to stop publishing.
    pub fn set_state(&mut self, state: &PlaybackState) -> Result<()> {
        match self.backend.as_mut() {
            Some(backend) => backend.set_state(state),
            None => Ok(()),
        }
    }

    /// Detaches early, releasing the D-Bus name or the SMTC and Now Playing
    /// registrations.
    ///
    /// Idempotent, and called automatically on drop; you only need it to
    /// release the controls before the handle itself goes out of scope.
    pub fn detach(&mut self) {
        if let Some(mut backend) = self.backend.take() {
            backend.stop();
        }
    }
}

impl Drop for MediaControls {
    fn drop(&mut self) {
        self.detach();
    }
}

impl fmt::Debug for MediaControls {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaControls")
            .field("attached", &self.backend.is_some())
            .finish()
    }
}
