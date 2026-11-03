# HD packs

An HD pack replaces the original art with pictures at twice the size. ReTzar
draws them in place of the originals: an object keeps its size and
position on the map, only its picture is sharper. Any picture a pack does not
have stays original, so a pack can cover a few buildings or the whole game.

ReTzar ships no pack. You can make your own:

- a **personal pack** made from your copy of Tzar with an AI upscaler
  ([below](#making-a-personal-pack-with-ai-upscaling)). It is made from the
  game's copyrighted art: keep it for yourself and **do not share it**;
- a **pack of your own art**, drawn or rendered by you, which you can share
  under any licence you choose
  ([below](#making-a-pack-of-your-own-art-to-share)).

## Using a pack

1. Put the pack's folder in `hd-packs/` beside ReTzar's settings
   (`settings.json`): next to `ReTzar.exe` for the Windows release, next to
   the executable on Linux and macOS (or the folder `--root` names).
2. In **Preferences → Graphics**, choose it in **HD pack**. **HD terrain**
   also takes the pack's ground, if it has one.
3. The pack is drawn from the next game you load or start.

For the full detail set **Map pixels** to **Original**: the other choices
draw the map at one texel per game pixel, which throws the extra detail
away. Packs show best when the map is zoomed to 2× or closer (a 1440p or
larger screen at the default zoom).

`RETZAR_HD=<pack folder>` and `RETZAR_HD_TERRAIN=0|1` take the place of
the settings, for testing.

## The pack format

```
hd-packs/
  my-pack/
    pack.ini
    IMAGES/BUILDING/EURO CASTLE.RLE.png
    IMAGES/UNIT/EURO KNIGHT.RLE.png
    IMAGES/UNIT/EURO KNIGHT.RLE.PLAYER.png
    IMAGES/TERRAIN/SUMMER/TREES.BMP.png
    IMAGES/TERRAIN/SUMMER/22.BMP.png
    ...
```

### pack.ini

```ini
[pack]
name = My pack
author = Your name
license = CC-BY-4.0
description = What it covers, how it was made.
; How many times larger the pictures are. ReTzar draws packs of 2.
scale = 2
; 1 if the pack has ground atlases (TERRAIN/<biome>/NN.BMP).
terrain = 1
```

A folder without `pack.ini`, or with another `scale`, is not offered.

### Pictures

- **Name**: the archive's path of the original sheet, in capitals, plus
  `.png`: the sheet `IMAGES/BUILDING/EURO CASTLE.RLE` is
  `IMAGES/BUILDING/EURO CASTLE.RLE.png`. `retzar-tools --hd-export` writes
  every original under exactly these names (see below).
- **Size**: exactly twice the original sheet's width and height. A picture
  of another size is ignored, and the original drawn.
- **Layout**: the same as the exported original. A sheet holds all of an
  object's frames, stacked in the exported order; draw each frame where the
  original has it, at twice the size.
- **Format**: PNG, RGBA. Transparency is the alpha channel; soft edges are
  fine (they are smoothed when the map is zoomed out).
- **Not needed**: shadows (`*_S`, `* SHADOW`) are the engine's 50%
  darkening of the ground, drawn from the originals.

### Kingdom colours

Units, banners and some buildings show the colour of their kingdom. In the
originals those parts are painted in sixteen light greys, which the game
replaces with the kingdom's shades. A pack does the same:

- the picture paints the kingdom parts in **light greys**, brighter for
  lighter shades, as the exported original does;
- beside it, `<NAME>.PLAYER.png` (same size) marks them: white where a pixel
  is a kingdom part, black elsewhere, grey in between for soft edges.

ReTzar recolours the greys for each kingdom: a pixel's brightness picks the
shade, the mask how much of it. `--hd-export` writes the masks of the
originals in `export/player/`.

### Terrain

Ground atlases are `IMAGES/TERRAIN/<biome>/NN.BMP.png` (`N8.BMP` are cliffs,
drawn like objects). They are opaque. The transition atlases (two different
digits) carry pure magenta (`#FF00FF`) and green (`#00FF00`) markers that
choose between the two terrains; ReTzar takes the markers from the
originals and the pixels from the pack, so a pack's transition atlas only
needs good pixels around them (the exported originals show the markers as
transparent).

## The exported originals

```sh
cargo run --release --no-default-features --bin retzar-tools -- \
    --source "/path/to/Tzar" --hd-export export
```

writes every sheet ReTzar can replace, at its original size, under the
names a pack uses, and the kingdom masks in `export/player/`. It is the
template for a pack: what to draw, where, and at half what size. It is the
game's art: use it for reference and for personal packs, do not share it.

## Making a personal pack with AI upscaling

`tools/hd-pack/make-pack.sh` makes a pack from your copy of Tzar with
[Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) (ncnn-vulkan build,
BSD-3-Clause), on any Vulkan GPU.

Needs: Rust, Python 3 with numpy and Pillow, `curl`, `unzip`, about 10 GB of
disk, and some patience (about an hour or two for the whole game on a recent
GPU).

```sh
tools/hd-pack/make-pack.sh --tzar "/path/to/Tzar" --work ~/hd-work \
    --install /path/to/ReTzar --gpu 1
```

- `--gpu` picks the Vulkan device; the upscaler lists them when it starts
  (an integrated GPU is often device 0).
- `--install` links the finished pack into that ReTzar's `hd-packs/`.
- Run it again to resume: it only does what is missing.

What it does, step by step (each a script in `tools/hd-pack/`):

1. **Export** the originals (`retzar-tools --hd-export`).
2. **Prepare** (`prep.py`): fill each sprite's transparent pixels with the
   colours of its edges, so the upscaler draws no fringes of the magenta key,
   and split the transparency and the kingdom masks into images of their
   own.
3. **Upscale** 4×: colours with `realesrgan-x4plus`, masks with the faster
   `realesr-animevideov3`.
4. **Check** (`check.py`): GPU faults ("device lost") can spoil whole runs;
   every result is compared with its source and redone one by one.
   `fix_small.py` resizes the few masks too small for the model.
5. **Terrain mix** (`terrain_mix.py`): x4plus smooths away the texture of
   water and grass, so those atlases come from animevideov3.
6. **Combine** (`combine.py`): bring everything to 2× (Lanczos), restore
   anti-aliased transparency from the upscaled masks, correct each sheet's
   average colour (the upscaler darkens dithered art), add the original's
   fine texture back to the ground, and write the pack with its
   `pack.ini`.

## Making a pack of your own art to share

A pack of art you made yourself can be shared, under a licence of your
choice (for example CC-BY-4.0 or CC0). To make one:

1. Export the originals (above) to see which sheets exist, their sizes,
   layouts and frame order.
2. Draw or render your own pictures at **twice** each sheet's size, in the
   same layout, and save them under the same names. Cover as much or as
   little as you like: missing pictures stay original.
3. For units and other kingdom-coloured art, paint the kingdom parts in
   light greys and add the `.PLAYER.png` mask.
4. Write `pack.ini` with your name, the licence and a description.
5. Share the pack folder, with **only your own pictures** in it.

Do not include anything made from the game's art: not the exported
originals, not AI upscales or traces of them, not edits of them. Those stay
the property of Tzar's rights holders (Haemimont Games, Take-Two
Interactive, FX Interactive), whatever tool made them and however old the
game is.
