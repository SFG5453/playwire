// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

use std::sync::Arc;

use crate::config::PlayerConfig;
use crate::error::Result;
use crate::state::{Event, PlaybackState};

/// Called from whichever thread the OS delivers its controls on, so every
/// backend needs its handler to be `Send + Sync`.
pub(crate) type Emitter = Arc<dyn Fn(Event) + Send + Sync + 'static>;

pub(crate) trait Backend: Send {
    fn set_state(&mut self, state: &PlaybackState) -> Result<()>;
    fn stop(&mut self);
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
mod mpris;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub(crate) fn start(config: &PlayerConfig, emit: Emitter) -> Result<Box<dyn Backend>> {
    Ok(Box::new(mpris::MprisBackend::start(config, emit)?))
}

#[cfg(target_os = "windows")]
pub(crate) fn start(config: &PlayerConfig, emit: Emitter) -> Result<Box<dyn Backend>> {
    Ok(Box::new(windows::SmtcBackend::start(config, emit)?))
}

#[cfg(target_os = "macos")]
pub(crate) fn start(config: &PlayerConfig, emit: Emitter) -> Result<Box<dyn Backend>> {
    Ok(Box::new(macos::NowPlayingBackend::start(config, emit)?))
}

#[cfg(not(any(
    all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ),
    target_os = "windows",
    target_os = "macos"
)))]
pub(crate) fn start(_config: &PlayerConfig, _emit: Emitter) -> Result<Box<dyn Backend>> {
    Err(crate::error::Error::Unsupported)
}
