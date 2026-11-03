#!/usr/bin/env bash
# Make a personal HD pack from your own copy of Tzar with Real-ESRGAN.
# See docs/hd-packs.md. The result is made from the game's art: keep it for
# yourself, do not share it.
set -euo pipefail

usage() {
    cat <<'EOT'
Usage: tools/hd-pack/make-pack.sh --tzar DIR [options]

  --tzar DIR      your Tzar installation (the folder with Maps)
  --work DIR      working folder, about 10 GB (default: ./hd-work)
  --name NAME     the pack's folder name (default: tzar-ai-hd)
  --install DIR   ReTzar's settings folder: the pack is linked into its
                  hd-packs/ (default: do not install)
  --gpu N         Vulkan device for the upscaler (default: 0; the upscaler
                  lists the devices when it starts)

Steps: export the sheets (retzar-tools --hd-export), prepare them, upscale
colours with realesrgan-x4plus and masks with realesr-animevideov3, redo any
spoiled by a GPU fault, upscale water and grass with animevideov3, combine at
2x and write pack.ini. Running it again only does what is missing.
EOT
}

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
tzar="" work="$PWD/hd-work" name=tzar-ai-hd install="" gpu=0
while [ $# -gt 0 ]; do
    case $1 in
        --tzar) tzar=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --name) name=$2; shift 2 ;;
        --install) install=$2; shift 2 ;;
        --gpu) gpu=$2; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done
[ -n "$tzar" ] || { usage >&2; exit 2; }
mkdir -p "$work"
work=$(cd "$work" && pwd)
py() { python3 -I "$@"; }
step() { printf '\n== %s\n' "$*" >&2; }

step "Tools"
python3 -c "import numpy, PIL" 2>/dev/null || { echo "needs python3 with numpy and Pillow" >&2; exit 1; }
cargo build --release --no-default-features --bin retzar-tools --manifest-path "$repo/Cargo.toml" >&2
tools="$repo/target/release/retzar-tools"
"$here/setup-esrgan.sh" "$work/tools"
esrgan="$work/tools/realesrgan/realesrgan-ncnn-vulkan"
models="$work/tools/realesrgan/models"
model() { [ "$1" = rgb ] && echo realesrgan-x4plus || echo realesr-animevideov3; }

step "Export"
"$tools" --source "$tzar" --hd-export "$work/export"
mkdir -p "$work/inis"
for b in COMMON DESERT GRASS SNOW SUMMER; do
    "$tools" --source "$tzar" --extract "IMAGES/TERRAIN/$b.TERRAIN.INI" \
        --to "$work/inis/$b.TERRAIN.INI" >/dev/null 2>&1 || true
done

step "Prepare"
py "$here/prep.py" "$work"

step "Upscale"
for kind in rgb mask player; do
    mkdir -p "$work/work/${kind}4"
    n=$(find "$work/work/todo/$kind" -name '*.png' | wc -l)
    [ "$n" -eq 0 ] && continue
    echo "$n $kind" >&2
    "$esrgan" -i "$work/work/todo/$kind" -o "$work/work/${kind}4" -m "$models" \
        -n "$(model "$kind")" -s 4 -f png -g "$gpu" >/dev/null 2>&1 || true
done

step "Check"
for pass in 1 2 3; do
    mapfile -t bad < <(py "$here/check.py" "$work" 2>/dev/null)
    echo "pass $pass: ${#bad[@]} to redo" >&2
    [ "${#bad[@]}" -eq 0 ] && break
    for line in "${bad[@]}"; do
        kind=${line%%$'\t'*}
        file=${line#*$'\t'}
        "$esrgan" -i "$work/work/$kind/$file" -o "$work/work/${kind}4/$file" -m "$models" \
            -n "$(model "$kind")" -s 4 -g "$gpu" -t 128 >/dev/null 2>&1 || true
    done
done
py "$here/fix_small.py" "$work"

step "Terrain mix"
mkdir -p "$work/work/rgb4_anime"
while read -r file; do
    [ -s "$work/work/rgb4_anime/$file" ] && continue
    "$esrgan" -i "$work/work/rgb/$file" -o "$work/work/rgb4_anime/$file" -m "$models" \
        -n realesr-animevideov3 -s 4 -g "$gpu" >/dev/null 2>&1 || echo "failed: $file" >&2
done < <(py "$here/terrain_mix.py" "$work")

step "Combine"
py "$here/combine.py" "$work"
cat > "$work/pack/pack.ini" <<EOT
[pack]
name = Tzar AI HD (personal)
author = $(whoami)
license = Personal use only: made from the original game's art. Do not share.
description = The original art upscaled with Real-ESRGAN (x4plus; animevideov3 for masks, water and grass).
scale = 2
terrain = 1
EOT

if [ -n "$install" ]; then
    step "Install"
    mkdir -p "$install/hd-packs"
    ln -sfn "$work/pack" "$install/hd-packs/$name"
    echo "installed: $install/hd-packs/$name -> $work/pack" >&2
fi
du -sh "$work/pack" >&2
echo "Done. Choose it in Preferences > Graphics > HD pack." >&2
