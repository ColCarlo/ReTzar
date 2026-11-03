"""List upscaled files that are missing or broken (wrong size, or whose
average brightness no longer matches the source), as 'kind<TAB>name'.
Colours are allowed the upscaler's usual darkening of dithered art; a broken
result (a GPU fault) comes out black or blank."""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# Tall sprite sheets at 4x pass 100 million pixels; they are our own files.
Image.MAX_IMAGE_PIXELS = None

work = Path(sys.argv[1]) / "work"
# Results looked at and kept (scripts/fix_small.py).
accepted = work / "accepted.txt"
skip = set(accepted.read_text().splitlines()) if accepted.exists() else set()
for kind in ("rgb", "mask", "player"):
    for f in sorted((work / kind).glob("*.png")):
        if f"{kind}\t{f.name}" in skip:
            continue
        up_path = work / f"{kind}4" / f.name
        if not up_path.exists():
            print(f"{kind}\t{f.name}")
            continue
        src = np.asarray(Image.open(f).convert("L"), dtype=np.float32)
        up = np.asarray(Image.open(up_path).convert("L"), dtype=np.float32)
        h, w = src.shape
        if up.shape != (h * 4, w * 4):
            print(f"{kind}\t{f.name}")
            continue
        small = up.reshape(h, 4, w, 4).mean((1, 3))
        if src.std() > 1 and np.corrcoef(src.ravel(), small.ravel())[0, 1] < 0.8:
            print(f"{kind}\t{f.name}")
        elif abs(small.mean() - src.mean()) > 25:
            print(f"{kind}\t{f.name}")
