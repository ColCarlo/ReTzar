# ReTzar

A fan remake of the classic RTS **Tzar**, rebuilt from scratch in Rust and
Bevy and kept as close to the original as possible.

![A castle, an army and a walled town](screenshots/1-castle-and-army.jpg)

- All three civilizations, random maps and the campaigns of several
  editions: The Burden of the Crown, Excalibur and King Arthur, Genghis Khan,
  The Domains of Magic, El Cid and the Reconquista
- The original AI, mission scripts, day and night, weather, fog of war
- Runs smoothly on modern Windows, with crisp scaling up to 4K
- Seven languages: English, Italian, French, German, Spanish, Polish, Portuguese
- A few optional extras (smooth zoom, night lights, CRT effect…), all off by default

## Play

1. Download the zip of the
   [latest release](https://github.com/ColCarlo/ReTzar/releases/latest) and
   unzip it somewhere of yours, such as the Desktop or Documents.
2. Run `ReTzar.exe` in the `ReTzar` folder. The first time, it asks for the
   folder where Tzar is installed.

Windows may say that it protected your PC, since the game isn't signed:
click **More info**, then **Run anyway**.

| | |
|---|---|
| ![Winter river](screenshots/2-winter-river.jpg) | ![Two kingdoms](screenshots/3-two-kingdoms.jpg) |
| ![Random map](screenshots/4-random-map.jpg) | ![Main menu](screenshots/5-main-menu.png) |

## What you need

- Windows 10 or 11 (64-bit), 4 GB of RAM and about 600 MB of disk space
- A graphics card with an up-to-date Vulkan driver
- A copy of Tzar, any edition

## Build it yourself

Install [Rust](https://rustup.rs) and Visual Studio 2022 Build Tools (C++
desktop workload), put the `ReTzar.dat` of the release zip in a `release`
folder here and run `build-release.cmd`.

### Experimental macOS support

The renderer selects Metal on macOS and Vulkan on other platforms. macOS
plays sound effects, voices and movie soundtracks through Core Audio, and
MIDI music through Apple's General MIDI synthesizer. Original DDV movies
(including those in the GOG release) use the existing Rust decoder.
MP4/MPEG decoding and MP4 conversion use macOS's AVFoundation framework.
No extra media software is needed. macOS support is experimental and has
been tried on an Apple M2 Pro.

With the Rust toolchain specified in `rust-toolchain.toml` installed, run
`cargo build --locked --bin retzar` on a Mac. Launch with
`cargo run --locked --bin retzar`: a native folder picker asks for your
original Tzar installation once and remembers it in `settings.json`.
The original files stay in place and must remain accessible. You can also
pass `--source "/path/to/Tzar"` to select an installation explicitly.

The `retzar-tools` data-pack builder can also run on macOS. Its H.264
encoder uses a bitrate estimate for the requested quality; output is not
byte-identical to Windows encoding. First-run setup reads your installation
directly and does not need to build a pack.

## License

Code under [GPL-3.0-or-later](LICENSE). Tzar and all its game data are
© and ™ Haemimont Games, Take-Two Interactive and FX Interactive.
ReTzar is an unofficial fan project.
