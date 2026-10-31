#!/usr/bin/env bash
# Builds flockswitch.app around a compiled binary.
#   packaging/macos/make-app.sh <binary> <out-dir>
# The version comes from Cargo.toml. The bundle is ad-hoc signed (no Apple developer
# certificate), so Gatekeeper asks once before the first launch.
set -euo pipefail

bin="$1"
out="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)"
app="$out/flockswitch.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/flockswitch"
sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"

iconset="$(mktemp -d)/AppIcon.iconset"
(cd "$root" && cargo run --quiet --release --example iconset -- "$iconset")
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

codesign --force --sign - "$app"
echo "$app ($version)"
