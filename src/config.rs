// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! How your player identifies itself to the OS.

/// Identity and per-platform requirements, supplied once when the controls are
/// created.
#[derive(Clone, Debug)]
pub struct PlayerConfig {
    /// Human-readable name shown to users. MPRIS `Identity`.
    pub identity: String,
    /// The last component of the D-Bus name: `Foo` becomes
    /// `org.mpris.MediaPlayer2.Foo`. Must be a valid D-Bus name element --
    /// ASCII alphanumerics and underscores, not starting with a digit.
    ///
    /// Linux only.
    pub bus_name: String,
    /// Your `.desktop` file's basename without the extension, e.g.
    /// `dev.sfg.orchard`. GNOME and KDE use MPRIS `DesktopEntry` to resolve the
    /// player to its icon; leave it empty and the media widget shows a generic
    /// placeholder.
    ///
    /// Linux only.
    pub desktop_entry: String,
    /// Object-path prefix for `mpris:trackid`. The track id is appended, so the
    /// default yields `/org/mpris/MediaPlayer2/Track/<id>`.
    ///
    /// Must be a valid D-Bus object path with no trailing slash. Linux only.
    pub track_id_prefix: String,
    /// MPRIS `SupportedUriSchemes`.
    pub supported_uri_schemes: Vec<String>,
    /// MPRIS `SupportedMimeTypes`.
    pub supported_mime_types: Vec<String>,
    /// The window to attach SMTC to, from your windowing library's raw handle.
    ///
    /// **Required on Windows**, ignored elsewhere.
    pub hwnd: Option<u64>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            identity: "Media Player".to_string(),
            bus_name: "MediaPlayer".to_string(),
            desktop_entry: String::new(),
            track_id_prefix: "/org/mpris/MediaPlayer2/Track".to_string(),
            supported_uri_schemes: vec!["http".to_string(), "https".to_string()],
            supported_mime_types: Vec::new(),
            hwnd: None,
        }
    }
}

impl PlayerConfig {
    /// A config with `identity` and `bus_name` both set to `name`, and every
    /// other field defaulted.
    ///
    /// `name` must be a valid D-Bus name element, since it becomes the bus name:
    /// ASCII alphanumerics and underscores, not starting with a digit. Use the
    /// builder methods to set the fields that matter for your platform --
    /// [`desktop_entry`](Self::desktop_entry) on Linux, [`hwnd`](Self::hwnd) on
    /// Windows.
    ///
    /// ```
    /// # use playwire::PlayerConfig;
    /// let config = PlayerConfig::new("Fauxplayer")
    ///     .desktop_entry("com.example.fauxplayer");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            identity: name.clone(),
            bus_name: name,
            ..Self::default()
        }
    }

    /// Sets the `.desktop` file basename, without the extension.
    ///
    /// Linux only, and worth setting: it is how GNOME and KDE find your icon.
    /// See [`PlayerConfig::desktop_entry`](Self#structfield.desktop_entry).
    pub fn desktop_entry(mut self, entry: impl Into<String>) -> Self {
        self.desktop_entry = entry.into();
        self
    }

    /// Sets the object-path prefix for `mpris:trackid`.
    ///
    /// Namespace this to your application so two players cannot collide in a
    /// client's metadata cache. Linux only.
    pub fn track_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.track_id_prefix = prefix.into();
        self
    }

    /// Sets the URI schemes the player can open, published as MPRIS
    /// `SupportedUriSchemes`. Defaults to `http` and `https`.
    pub fn supported_uri_schemes(mut self, schemes: Vec<String>) -> Self {
        self.supported_uri_schemes = schemes;
        self
    }

    /// Sets the MIME types the player can handle, published as MPRIS
    /// `SupportedMimeTypes`. Defaults to empty.
    pub fn supported_mime_types(mut self, types: Vec<String>) -> Self {
        self.supported_mime_types = types;
        self
    }

    /// Sets the window SMTC attaches to.
    ///
    /// **Required on Windows**, ignored elsewhere. Pass the `HWND` as an
    /// integer, from your windowing library's raw window handle. Constructing
    /// [`MediaControls`](crate::MediaControls) on Windows without it fails with
    /// [`Error::Config`](crate::Error::Config).
    pub fn hwnd(mut self, hwnd: u64) -> Self {
        self.hwnd = Some(hwnd);
        self
    }
}
