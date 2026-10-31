#!/bin/sh
# Renders the application icons from the masters icon-dark.svg and icon-light.svg (1024 px):
#   icon.png        512 px, dark: window, Linux desktop entry, AppImage, macOS bundle, web
#   icon-light.png  512 px, light: window on light themes, web on light browsers
#   icon.ico        dark, 16-256 px: the Windows executable (apps/wavestorm/build.rs)
# Needs rsvg-convert (librsvg) and ImageMagick 7. Run from anywhere after changing a master.
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
rsvg-convert -w 512 -h 512 icon-dark.svg -o icon.png
rsvg-convert -w 512 -h 512 icon-light.svg -o icon-light.png
for s in 16 24 32 48 64 128 256; do
  rsvg-convert -w $s -h $s icon-dark.svg -o "$tmp/$s.png"
done
magick "$tmp/16.png" "$tmp/24.png" "$tmp/32.png" "$tmp/48.png" "$tmp/64.png" "$tmp/128.png" "$tmp/256.png" icon.ico
echo "wrote icon.png icon-light.png icon.ico"
