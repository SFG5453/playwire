# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0]

- Publish the Windows AppUserModelID through SMTC so the media flyout can
  resolve the app's name and icon. `PlayerConfig::app_media_id` can supply it,
  or the backend uses an explicit process AppUserModelID when available.
- `PlayerConfig` gains a public field, so callers using a struct literal must
  add `app_media_id` (or use `..PlayerConfig::default()`).

## [1.0.0]

Initial release. The API is considered stable; [`Event`] is `#[non_exhaustive]`
so new controls can be added without a breaking change.

- MPRIS v2 backend on zbus 5, with no libdbus linkage.
- System Media Transport Controls backend on windows-rs 0.62.
- Now Playing / Remote Command Center backend on objc2.
- Shuffle and repeat on all three platforms.
- Capability flags (`can_go_next`, `can_go_previous`, `can_seek`) driven from
  published state rather than hardcoded.
- Configurable `mpris:trackid` prefix, plus `xesam:url`, `DesktopEntry` and full
  artist lists on MPRIS.
- `MPNowPlayingInfoPropertyPlaybackRate` on macOS, so Control Center's scrubber
  advances.
- Full rustdoc coverage, with per-item notes on which platforms honour what.

[`Event`]: https://docs.rs/playwire/latest/playwire/enum.Event.html
