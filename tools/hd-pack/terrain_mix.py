"""The ground atlases for the texture-keeping model of the terrain mix.

Reads the biomes' TERRAIN.INI files (extracted into <work>/inis/ by
make-pack.sh) and prints the flattened names of the atlases of a single
terrain type whose name says water or grass: those come from animevideov3,
which keeps their ripples and blades; the rest stay with x4plus
(combine.py adds the original's fine texture back to both).
"""
import sys
from pathlib import Path

root = Path(sys.argv[1])
for ini in sorted((root / "inis").glob("*.TERRAIN.INI")):
    biome, section, folder, kinds = ini.name.split(".")[0], "", None, []
    for line in ini.read_text(encoding="latin-1").splitlines():
        line = line.split(";")[0].strip()
        if line.startswith("["):
            section = line.strip("[]").lower()
        elif "=" in line:
            key, value = (p.strip() for p in line.split("=", 1))
            if section == "terrain" and key.lower() == "dir":
                folder = value.upper()
            elif section == "terrainname" and key.isdigit():
                if any(w in value.lower() for w in ("water", "grass")):
                    kinds.append(key)
    for k in kinds:
        name = f"IMAGES%%TERRAIN%%{folder or biome}%%{k}{k}.BMP.png"
        if (root / "work" / "rgb" / name).exists():
            print(name)
