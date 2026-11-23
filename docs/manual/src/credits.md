# Credits

Thank you to the people whose work this plugin is built on.

## Code we use

| Project | Authors | What it does here | Licence |
| --- | --- | --- | --- |
| [OxiGuitar's plugin SDK](https://github.com/mateusrdp/oxiguitar) (`oxiguitar-plugin`, and `oxiguitar-host` and `oxiguitar-plugin-test` for the tests) | Mateus Rocha de Paula and the OxiGuitar contributors | The plugin interface, and OxiGuitar's own host to test against | MIT OR Apache-2.0 |
| [Symphonia](https://github.com/pdeljanov/Symphonia) | Philip Deljanov and contributors | Decoding WAV, FLAC and MP3 | MPL-2.0 |
| [opus-pure](https://github.com/stephenberry/opus-pure) | Stephen Berry and restsend.com; a port of libopus (Xiph.Org, Skype Limited, Octasic and others) | Decoding Opus | BSD-3-Clause |
| [Rubato](https://github.com/HEnquist/rubato) | Henrik Enquist | Resampling to the sound card's rate | MIT OR Apache-2.0 |
| [timestretch](https://github.com/robmorgan/timestretch-rs) | Rob Morgan and contributors | Practice speeds with the pitch kept | MIT |
| [rfd](https://github.com/PolyMeilex/rfd) | Bartłomiej Maryńczak and contributors | The system's file dialog | MIT |
| [pathdiff](https://github.com/Manishearth/pathdiff) | Manish Goregaokar | Paths relative to the score | MIT OR Apache-2.0 |
| [rusty_flac](https://github.com/Remade-With-Rust/rusty_flac) and [rusty_mp3](https://github.com/Remade-With-Rust/remade_ffmpeg_rs) | Mata Network | Making test files (tests only) | Apache-2.0 |
| [Rust](https://www.rust-lang.org/) and its standard library | The Rust Project | Everything else | MIT OR Apache-2.0 |

## Tools

| Tool | Use |
| --- | --- |
| [mdBook](https://github.com/rust-lang/mdBook) | Builds this manual |
| [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) | Checks the licences of everything we build with |

## Thanks

To the Rust audio community, whose decoders, resamplers and stretchers made this plugin a matter of putting good pieces together, and to everyone who lays a clip on a score and tells us how it went.
