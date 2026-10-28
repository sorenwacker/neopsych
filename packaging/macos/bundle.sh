#!/usr/bin/env bash
# Builds neopsych.app for Apple Silicon and Intel and packs it into a .dmg.
# Run on macOS from the repository root: packaging/macos/bundle.sh 0.2.0
set -euo pipefail

version="${1:?usage: bundle.sh <version>}"
stage="dist/dmg"
app="$stage/neopsych.app"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

rm -rf dist
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/neopsych" \
    target/aarch64-apple-darwin/release/neopsych \
    target/x86_64-apple-darwin/release/neopsych

# The icon, in every size macOS asks for.
iconset="$(mktemp -d)/neopsych.iconset"
mkdir -p "$iconset"
for size in 16 32 64 128 256; do
    sips -z "$size" "$size" assets/neopsych.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    if [ "$double" -le 256 ]; then
        sips -z "$double" "$double" assets/neopsych.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    fi
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/neopsych.icns"

sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# Drag-to-Applications disk image.
ln -s /Applications "$stage/Applications"
hdiutil create -volname neopsych -srcfolder "$stage" -ov -format UDZO "dist/neopsych-$version-macos.dmg"
