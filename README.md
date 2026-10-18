# ReTzar

A fan remake of the classic RTS **Tzar**, rebuilt from scratch in Rust and
Bevy and kept as close to the original as possible.

![A castle, an army and a walled town](screenshots/1-castle-and-army.jpg)

- All three civilizations, random maps and the campaigns of several
  editions: The Burden of the Crown, Excalibur and King Arthur, Genghis Khan,
  The Domains of Magic
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

- Windows 10 or 11 (64-bit), 4 GB of RAM and about 500 MB of disk space
- A graphics card with an up-to-date Vulkan driver
- A copy of Tzar, any edition

## Build it yourself

Install [Rust](https://rustup.rs) and Visual Studio 2022 Build Tools (C++
desktop workload), put the `ReTzar.dat` of the release zip in a `release`
folder here and run `build-release.cmd`.

## License

Code under [GPL-3.0-or-later](LICENSE). Tzar and all its game data are
© and ™ Haemimont Games, Take-Two Interactive and FX Interactive.
ReTzar is an unofficial fan project.
