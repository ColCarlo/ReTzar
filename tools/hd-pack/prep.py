"""Prepare exported sprite sheets for Real-ESRGAN.

For every sheet under export/ (export/player/ holds the kingdom-colour masks):
- work/rgb: the colours, transparent pixels filled by colours bled in from the
  sprite's edges, so the upscaler never sees the magenta key and draws no
  fringes;
- work/mask: the transparency as a grey image;
- work/player: the kingdom-colour mask, for sheets that have one.
Paths are flattened ('/' -> '%%') because the upscaler reads one folder.
Sheets already prepared are skipped; new ones are also listed in work/todo/
for the upscaler.
"""
import shutil
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# Tall sprite sheets at 4x pass 100 million pixels; they are our own files.
Image.MAX_IMAGE_PIXELS = None

root = Path(sys.argv[1])
export, work = root / "export", root / "work"
players = export / "player"
for kind in ("rgb", "mask", "player"):
    (work / kind).mkdir(parents=True, exist_ok=True)
    shutil.rmtree(work / "todo" / kind, ignore_errors=True)
    (work / "todo" / kind).mkdir(parents=True, exist_ok=True)


def bleed(rgb: np.ndarray, solid: np.ndarray, steps: int = 24) -> np.ndarray:
    """Grow the solid pixels' colours into the transparent ones."""
    rgb = rgb.astype(np.float32)
    filled = solid.copy()
    for _ in range(steps):
        if filled.all():
            break
        total = np.zeros_like(rgb)
        count = np.zeros(filled.shape, np.float32)
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)):
            f = np.roll(filled, (dy, dx), (0, 1))
            c = np.roll(rgb, (dy, dx), (0, 1))
            total += c * f[..., None]
            count += f
        grow = ~filled & (count > 0)
        rgb[grow] = total[grow] / count[grow][:, None]
        filled |= grow
    if not filled.all():
        rgb[~filled] = rgb[solid].mean(axis=0) if solid.any() else 0
    return rgb.clip(0, 255).astype(np.uint8)


def save(kind: str, name: str, image: Image.Image) -> None:
    image.save(work / kind / name)
    shutil.copy(work / kind / name, work / "todo" / kind / name)


files = [f for f in sorted(export.rglob("*.png")) if players not in f.parents]
new = 0
for f in files:
    rel = f.relative_to(export)
    flat = rel.as_posix().replace("/", "%%")
    player = players / rel
    if (work / "rgb" / flat).exists() and (not player.exists() or (work / "player" / flat).exists()):
        continue
    a = np.asarray(Image.open(f).convert("RGBA"))
    solid = a[..., 3] > 127
    if not (work / "rgb" / flat).exists():
        save("rgb", flat, Image.fromarray(bleed(a[..., :3], solid)))
        save("mask", flat, Image.fromarray((solid * 255).astype(np.uint8)).convert("RGB"))
    if player.exists():
        save("player", flat, Image.open(player).convert("RGB"))
    new += 1
    if new % 100 == 0:
        print(f"prep {new} new", flush=True)
print(f"prep: {new} new of {len(files)} sheets", flush=True)
