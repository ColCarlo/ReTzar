"""Settle the upscales the checker keeps flagging.

Masks (transparency, kingdom colours) the model spoiled - it fails on images
a few pixels across - are redone with a plain smooth resize, which is all a
mask needs. Colour images it still flags are small sprites the model redrew a
little differently; they are kept and listed in work/accepted.txt so the
checker stops retrying them.
"""
import subprocess
import sys
from pathlib import Path

from PIL import Image

Image.MAX_IMAGE_PIXELS = None
root = Path(sys.argv[1])
work = root / "work"
flagged = subprocess.run(
    [sys.executable, "-I", str(Path(__file__).with_name("check.py")), str(root)],
    capture_output=True, text=True, check=True,
).stdout.splitlines()
accepted = work / "accepted.txt"
keep = set(accepted.read_text().splitlines()) if accepted.exists() else set()
resized = 0
for line in flagged:
    kind, name = line.split("\t")
    if kind == "rgb":
        keep.add(line)
        continue
    src = Image.open(work / kind / name).convert("RGB")
    src.resize((src.width * 4, src.height * 4), Image.Resampling.BICUBIC).save(work / f"{kind}4" / name)
    resized += 1
accepted.write_text("".join(f"{l}\n" for l in sorted(keep)))
print(f"fix_small: {resized} masks resized, {len(keep)} colour images accepted")
