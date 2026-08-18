# playwire

Cross-platform OS media controls and now-playing metadata, behind one API.

- **Linux** — MPRIS v2 over D-Bus (pure Rust via [zbus]; no libdbus to link)
- **Windows** — System Media Transport Controls
- **macOS** — `MPNowPlayingInfoCenter` / `MPRemoteCommandCenter`

You publish a [`PlaybackState`] whenever something changes, and receive an
[`Event`] whenever the user presses a media key or touches the OS media widget.

```rust,no_run
use std::time::Duration;
use playwire::{Capabilities, MediaControls, PlaybackState, PlayerConfig, Repeat, Track};

# fn main() -> playwire::Result<()> {
let config = PlayerConfig::new("Fauxplayer")
    .desktop_entry("com.example.fauxplayer")
    .track_id_prefix("/com/example/fauxplayer/track");

let mut controls = MediaControls::new(config, |event| {
    // Called from an OS thread: hand off, don't block.
    println!("{event:?}");
})?;

controls.set_state(&PlaybackState {
    track: Some(Track {
        id: "1234".to_string(),
        title: "When The Sun Hits".to_string(),
        artists: vec!["Slowdive".to_string()],
        album: "Souvlaki".to_string(),
        artwork_url: "https://example.com/art.jpg".to_string(),
        url: "https://example.com/track/1234".to_string(),
    }),
    playing: true,
    position: Duration::from_secs(42),
    duration: Some(Duration::from_secs(290)),
    volume: 1.0,
    repeat: Repeat::Off,
    shuffle: false,
    capabilities: Capabilities { can_go_next: true, can_go_previous: false, can_seek: true },
})?;
# Ok(())
# }
```

Controls detach on drop.

## Platform notes

`PlayerConfig::hwnd` is **required on Windows** — SMTC attaches to a window. Get
it from your windowing library's raw window handle (`HWND` as an integer).

On Linux, `desktop_entry` is what lets GNOME and KDE resolve your player to its
`.desktop` file and show your icon in the media widget. Without it you get a
generic placeholder.

On macOS the process needs a running `NSApplication` for Now Playing to appear.

## Feature support

Not every platform exposes every control. Publishing state that a backend cannot
represent is silently ignored rather than an error.

| | Linux | Windows | macOS |
|---|---|---|---|
| Play / Pause / PlayPause / Next / Previous | ✓ | ✓ | ✓ |
| Stop | ✓ | ✓ | ✓ |
| Seek to position | ✓ | ✓ | ✓ |
| Seek by amount | ✓ | ✓ (fixed 10s) | — |
| Volume | ✓ | — | — |
| Shuffle / repeat | ✓ | ✓ | ✓ |
| Capability flags | ✓ | ✓ | ✓ |
| Artwork | ✓ URL | ✓ URL | ✓ fetched |
| Raise / Quit / OpenUri | ✓ | — | — |

## Position updates

Publish position on every tick. Backends diff against the last snapshot, so
unchanged fields cost nothing: MPRIS `Position` is deliberately not a
change-signalling property (clients extrapolate from `Rate` and the `Seeked`
signal, which is emitted only on genuine discontinuities), and Windows and macOS
only rebuild their timeline when the track or duration actually changes.

## Prior art

[souvlaki] solves the same problem and was the reference for this crate's shape.
playwire exists because souvlaki has been unmaintained for over a year and its
backends are missing pieces that a real player needs: no shuffle or repeat on any
platform, capability flags hardcoded to `true`, `mpris:trackid` emitted as the
literal `/`, a single artist rather than the credited list, no `xesam:url`, no
`DesktopEntry` (and `HasTrackList` misspelled), no `stopCommand` on macOS, and no
`MPNowPlayingInfoPropertyPlaybackRate` — without which Control Center's scrubber
never advances. Its macOS backend also reads the seek position out of the private
`_positionTime` ivar rather than the public accessor.

No souvlaki code is vendored here; every backend is written against current
crates.

## License

MIT OR Apache-2.0, at your option.

[zbus]: https://crates.io/crates/zbus
[souvlaki]: https://github.com/Sinono3/souvlaki
