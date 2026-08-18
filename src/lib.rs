// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

#![doc = include_str!("../README.md")]
#![warn(missing_debug_implementations)]

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
/// Dropping it detaches the handlers and releases the D-Bus name or SMTC
/// registration.
pub struct MediaControls {
    backend: Option<Box<dyn platform::Backend>>,
}

impl MediaControls {
    /// Registers with the platform's media service and begins listening.
    ///
    /// `handler` is called from whichever thread the OS delivers controls on --
    /// a D-Bus worker on Linux, a WinRT pool thread on Windows, the main run
    /// loop on macOS -- so it must be `Send + Sync`. Do not block in it; send
    /// the event to your player's own loop.
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
    /// state is cheap and will not spam the bus or flicker the OS widget.
    pub fn set_state(&mut self, state: &PlaybackState) -> Result<()> {
        match self.backend.as_mut() {
            Some(backend) => backend.set_state(state),
            None => Ok(()),
        }
    }

    /// Detaches early. Idempotent, and called automatically on drop.
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
