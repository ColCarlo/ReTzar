# ReTzar

A fan remake of the classic RTS **Tzar**, rebuilt from scratch in Rust and
Bevy and kept as close to the original as possible.

![A castle, an army and a walled town](screenshots/1-castle-and-army.jpg)

- All three civilizations, random maps and the campaigns of several
  editions: The Burden of the Crown, Excalibur and King Arthur, Genghis Khan
- The original AI, mission scripts, day and night, weather, fog of war
- Runs smoothly on modern Windows, with crisp scaling up to 4K
- Seven languages: English, Italian, French, German, Spanish, Polish, Portuguese
- A few optional extras (smooth zoom, night lights, CRT effect…), all off by default

## Play

1. Download the zip of the
   [latest release](https://github.com/ColCarlo/ReTzar/releases/latest) and
   unzip it somewhere of yours, such as the Desktop or Documents.
2. Run `ReTzar.exe` in the `ReTzar` folder. The first time, it asks for the
   folder where Tzar is installed, to check that you own the game. After
   that it starts straight away.

Windows may say that it protected your PC, since the game isn't signed:
click **More info**, then **Run anyway**.

| | |
|---|---|
| ![Winter river](screenshots/2-winter-river.jpg) | ![Two kingdoms](screenshots/3-two-kingdoms.jpg) |
| ![Random map](screenshots/4-random-map.jpg) | ![Main menu](screenshots/5-main-menu.png) |

## What you need

- Windows 10 or 11 (64-bit), 4 GB of RAM (the game itself uses about half
  a gigabyte) and about 350 MB of disk space
- A graphics card with an up-to-date Vulkan driver: roughly NVIDIA GeForce
  GTX 600, AMD Radeon HD 7000, Intel HD Graphics 500 or newer
- A copy of Tzar, any edition: CD, GOG or Steam, in any language, modded
  or not. ReTzar already holds every campaign, voice and language; it only
  starts beside your copy.

## Build it yourself

1. Install [Rust](https://rustup.rs) and Visual Studio 2022 Build Tools
   (C++ desktop workload), with a few GB of free disk space.
2. Put the `ReTzar.dat` of the release zip in a `release` folder here and
   run `build-release.cmd`: you get the whole game in `release`.

Without that file, `build-release.cmd` makes one from the folder where Tzar
is installed, for example `build-release.cmd "C:\GOG Games\Tzar"`, with
the content of that edition.

## License

Code under [GPL-3.0-or-later](LICENSE). The game data belong to Haemimont
Games, Take-Two Interactive and FX Interactive, and are included with their
permission as long as ReTzar stays free: see [DATA-LICENSE](DATA-LICENSE.md).
Thanks to all of them. ReTzar is an unofficial fan project.
