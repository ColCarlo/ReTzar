#!/usr/bin/env bash
# Download Real-ESRGAN ncnn-vulkan v0.2.5.0 for Linux (the upscaler and its
# models, BSD-3-Clause) into $1/realesrgan, checking the archive's SHA-256.
set -euo pipefail
dest=${1:?usage: setup-esrgan.sh FOLDER}
url=https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-ubuntu.zip
sum=e5aa6eb131234b87c0c51f82b89390f5e3e642b7b70f2b9bbe95b6a285a40c96
if [ -x "$dest/realesrgan/realesrgan-ncnn-vulkan" ]; then
    exit 0
fi
mkdir -p "$dest"
zip="$dest/realesrgan.zip"
curl -fL --retry 3 -o "$zip" "$url"
echo "$sum  $zip" | sha256sum -c -
rm -rf "$dest/realesrgan"
mkdir -p "$dest/realesrgan"
unzip -q "$zip" -d "$dest/realesrgan"
rm "$zip"
chmod +x "$dest/realesrgan/realesrgan-ncnn-vulkan"
