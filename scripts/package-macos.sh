#!/usr/bin/env bash
# Build Drift for macOS as a universal app (Apple silicon and Intel in one) in a disk image:
#
#   dist/Drift-<version>-macos-universal.dmg
#
# The app holds drift-mcp too, the MCP server AI assistants use
# (docs/guide/ai/assistants.md), at Drift.app/Contents/MacOS/drift-mcp.
#
# Run it on a Mac with Xcode's command line tools and Rust:
#
#   scripts/package-macos.sh
#
# The app is signed "ad hoc" unless CODESIGN_IDENTITY names a Developer ID certificate in
# your keychain. An ad-hoc app isn't notarised, so macOS asks people to confirm the first
# time they open it (right-click the app, then Open).
set -euo pipefail

cd "$(dirname "$0")/.."
if [[ "$(uname)" != "Darwin" ]]; then
    echo "this script needs macOS (it uses lipo, iconutil, codesign and hdiutil)" >&2
    exit 1
fi

# The version, from the workspace manifest; the build number is its numeric part.
VERSION=$(awk '/^\[workspace\.package\]/{p=1} p && /^version/{gsub(/[" ]/,"",$0); sub(/version=/,""); print; exit}' Cargo.toml)
BUILD=${VERSION%%-*}
echo "Packaging Drift $VERSION"

export MACOSX_DEPLOYMENT_TARGET=11.0
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --bin drift --target aarch64-apple-darwin
cargo build --release --bin drift --target x86_64-apple-darwin
# Separate builds, so drift-mcp's dependencies can't change drift's features.
cargo build --release --bin drift-mcp --target aarch64-apple-darwin
cargo build --release --bin drift-mcp --target x86_64-apple-darwin

DIST=dist
APP="$DIST/Drift.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# One executable for both kinds of Mac.
lipo -create -output "$APP/Contents/MacOS/drift" \
    target/aarch64-apple-darwin/release/drift \
    target/x86_64-apple-darwin/release/drift
lipo -info "$APP/Contents/MacOS/drift"
lipo -create -output "$APP/Contents/MacOS/drift-mcp" \
    target/aarch64-apple-darwin/release/drift-mcp \
    target/x86_64-apple-darwin/release/drift-mcp
lipo -info "$APP/Contents/MacOS/drift-mcp"
"$APP/Contents/MacOS/drift-mcp" --version

# The icon, at every size macOS asks for, from the 1024 px drawing.
ICONSET=$(mktemp -d)/Drift.iconset
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size apps/native/assets/icon-mac.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double apps/native/assets/icon-mac.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$APP/Contents/Resources/Drift.icns" "$ICONSET"

sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" installer/macos/Info.plist > "$APP/Contents/Info.plist"
cp LICENSE.md THIRD-PARTY-NOTICES.txt "$APP/Contents/Resources/"

# drift-mcp first: it runs on its own, outside the app, so it needs a signature of its own.
codesign --force --options runtime --sign "${CODESIGN_IDENTITY:--}" "$APP/Contents/MacOS/drift-mcp"
codesign --force --deep --options runtime --sign "${CODESIGN_IDENTITY:--}" "$APP"
codesign --verify --deep --strict "$APP"

# A disk image with the app and a shortcut to Applications to drag it onto.
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST/Drift-$VERSION-macos-universal.dmg"
rm -f "$DMG"
hdiutil create -volname "Drift" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "wrote $DMG"
