// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

use std::fmt;

/// Everything that can go wrong talking to the platform's media service.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The platform's media service could not be reached: no session bus, no
    /// WinRT, and so on.
    Unavailable(String),
    /// Publishing state or metadata failed.
    Publish(String),
    /// A required piece of configuration was missing or malformed, such as an
    /// absent `hwnd` on Windows.
    Config(String),
    /// This platform has no media-controls integration.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(detail) => {
                write!(formatter, "media controls unavailable: {detail}")
            }
            Self::Publish(detail) => write!(formatter, "failed to publish state: {detail}"),
            Self::Config(detail) => write!(formatter, "invalid configuration: {detail}"),
            Self::Unsupported => {
                write!(formatter, "media controls are not supported on this platform")
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
