// Copyright 2026 SFG545
//
// Licensed under the MIT license <LICENSE-MIT> or the Apache License, Version
// 2.0 <LICENSE-APACHE>, at your option. This file may not be copied, modified,
// or distributed except according to those terms.

//! Publishes a fake track and prints every control the OS sends back.
//!
//! On Linux, try it with `playerctl -p Fauxplayer play-pause` or your desktop's
//! media widget. Windows needs a real window handle, so this example only does
//! something useful on Linux and macOS.

use std::time::Duration;

use playwire::{Capabilities, MediaControls, PlaybackState, PlayerConfig, Repeat, Track};

fn main() -> playwire::Result<()> {
    let config = PlayerConfig::new("Fauxplayer")
        .desktop_entry("com.example.fauxplayer")
        .track_id_prefix("/com/example/fauxplayer/track");

    let mut controls = MediaControls::new(config, |event| println!("event: {event:?}"))?;

    let mut state = PlaybackState {
        track: Some(Track {
            id: "when-the-sun-hits".to_string(),
            title: "When The Sun Hits".to_string(),
            artists: vec!["Slowdive".to_string()],
            album: "Souvlaki".to_string(),
            artwork_url: String::new(),
            url: "https://example.com/track/1".to_string(),
        }),
        playing: true,
        position: Duration::ZERO,
        duration: Some(Duration::from_secs(290)),
        volume: 1.0,
        repeat: Repeat::Off,
        shuffle: false,
        capabilities: Capabilities::default(),
    };

    controls.set_state(&state)?;
    println!("published; press media keys or use your desktop's widget (Ctrl-C to quit)");

    // A real player would publish from its own playback loop.
    loop {
        std::thread::sleep(Duration::from_secs(1));
        state.position += Duration::from_secs(1);
        controls.set_state(&state)?;
    }
}
