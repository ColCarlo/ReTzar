"""Build the HD pack from the upscaled colour images and masks.

Each 4x result is brought to 2x (Lanczos), the size ReTzar's pack expects
(retzar::world::HD_SCALE), and gets back its transparency from the upscaled
mask, with anti-aliased edges, and its average colours (the upscaler darkens
dithered art). Written to pack/<ARCHIVE PATH>.png; a sheet with kingdom
colours also gets pack/<ARCHIVE PATH>.PLAYER.png, its upscaled kingdom mask
(retzar::world::hd_kingdom_picture).

Ground atlases (TERRAIN/<biome>/NN.BMP) are the terrain mix: water and grass
from the texture-keeping animevideov3 (work/rgb4_anime) where it was run, the
rest from x4plus, both with the original's fine texture added back (detail
transfer), which the models smooth away.

An optional second argument rebuilds only the sheets whose path contains it.
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

# Tall sprite sheets at 4x pass 100 million pixels; they are our own files.
Image.MAX_IMAGE_PIXELS = None

root = Path(sys.argv[1])
only = sys.argv[2] if len(sys.argv) > 2 else ""
export, work, pack = root / "export", root / "work", root / "pack"
players = export / "player"
LANCZOS = Image.Resampling.LANCZOS


def edge(path: Path, size) -> np.ndarray:
    """An upscaled mask at `size`, its soft edge narrowed to about a pixel."""
    m = np.asarray(Image.open(path).convert("L").resize(size, LANCZOS), dtype=np.float32)
    return (((m / 255 - 0.5) * 2.5 + 0.5).clip(0, 1) * 255).astype(np.uint8)


done = missing = kingdom = 0
for f in sorted(export.rglob("*.png")):
    if players in f.parents:
        continue
    rel = f.relative_to(export)
    if only not in rel.as_posix():
        continue
    flat = rel.as_posix().replace("/", "%%")
    stem = rel.name.split(".")[0]
    ground = rel.parts[1] == "TERRAIN" and len(stem) == 2 and stem.isdigit() and stem[1] != "8"
    rgb_path, mask_path = work / "rgb4" / flat, work / "mask4" / flat
    if ground and (work / "rgb4_anime" / flat).exists():
        rgb_path = work / "rgb4_anime" / flat
    if not rgb_path.exists() or not mask_path.exists():
        missing += 1
        continue
    orig = np.asarray(Image.open(f).convert("RGBA"), dtype=np.float32)
    h, w = orig.shape[:2]
    size = (w * 2, h * 2)
    rgbf = np.asarray(Image.open(rgb_path).convert("RGB").resize(size, LANCZOS), dtype=np.float32)
    alpha = edge(mask_path, size)
    if ground:
        # The original's fine texture: a smooth enlargement minus its blur,
        # from the prepared picture, whose transition markers (pure magenta
        # and green) are already filled with the surrounding colours.
        smooth = Image.open(work / "rgb" / flat).convert("RGB").resize(size, LANCZOS)
        rgbf += np.asarray(smooth, dtype=np.float32) - np.asarray(
            smooth.filter(ImageFilter.GaussianBlur(1.5)), dtype=np.float32
        )
    solid_o, solid_n = orig[..., 3] > 127, alpha > 127
    if solid_o.any() and solid_n.any():
        rgbf += orig[..., :3][solid_o].mean(0) - rgbf[solid_n].mean(0)
    # Ground atlases are opaque: their transparent pixels were only the
    # transition markers, which ReTzar reads from the originals.
    if ground:
        alpha = np.full_like(alpha, 255)
    out = pack / rel
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(np.dstack([rgbf.clip(0, 255).astype(np.uint8), alpha]), "RGBA").save(out)
    player_path = work / "player4" / flat
    if (players / rel).exists():
        if player_path.exists():
            m = edge(player_path, size)
            Image.fromarray(m).convert("RGB").save(out.with_name(out.name[:-4] + ".PLAYER.png"))
            kingdom += 1
        else:
            missing += 1
    done += 1
print(f"pack: {done} sheets ({kingdom} with kingdom colours), {missing} missing")
