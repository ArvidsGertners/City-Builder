#!/usr/bin/env bash
# Builds CityBuilder.app for Apple Silicon (ADR-0038): release profile `dist`, Info.plist from
# packaging/macos, ad-hoc signature (not notarized). Output: target/bundle/CityBuilder.app
set -euo pipefail
cd "$(dirname "$0")/.."

target="aarch64-apple-darwin"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
cargo build -p cb_game --profile dist --target "$target"

app="target/bundle/CityBuilder.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$target/dist/cb_game" "$app/Contents/MacOS/cb_game"
sed "s/__VERSION__/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp LICENSE THIRD_PARTY_NOTICES.md crates/cb_game/assets/fonts/OFL.txt "$app/Contents/Resources/"
codesign --force --deep --sign - "$app"
echo "bundle: $app ($version)"
