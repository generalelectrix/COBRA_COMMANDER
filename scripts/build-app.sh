#!/bin/bash
# Build the complete "Cobra Commander.app" bundle and DMG from scratch.
# Usage: VERSION=2026.04.09-1 scripts/build-app.sh
#
# Prerequisites: brew install create-dmg poppler
#                pip install Pillow
#                rustup target add x86_64-apple-darwin aarch64-apple-darwin
set -e

VERSION="${VERSION:?VERSION env var is required (e.g. 2026.04.09-1)}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# --- Assemble icon ---
# Icons are checked in under resources/icons/. To regenerate from the source
# PDF (resources/emblem.pdf), run: python3 resources/generate_icons.py

echo "==> Converting PNGs to icns..."
ICNS="$PROJECT_DIR/resources/CobraCommander.icns"
ICONSET=$(mktemp -d)/CobraCommander.iconset
mkdir -p "$ICONSET"

ICON_DIR="$PROJECT_DIR/resources/icons"
cp "$ICON_DIR/icon_16.png"   "$ICONSET/icon_16x16.png"
cp "$ICON_DIR/icon_32.png"   "$ICONSET/icon_16x16@2x.png"
cp "$ICON_DIR/icon_32.png"   "$ICONSET/icon_32x32.png"
cp "$ICON_DIR/icon_64.png"   "$ICONSET/icon_32x32@2x.png"
cp "$ICON_DIR/icon_128.png"  "$ICONSET/icon_128x128.png"
cp "$ICON_DIR/icon_256.png"  "$ICONSET/icon_128x128@2x.png"
cp "$ICON_DIR/icon_256.png"  "$ICONSET/icon_256x256.png"
cp "$ICON_DIR/icon_512.png"  "$ICONSET/icon_256x256@2x.png"
cp "$ICON_DIR/icon_512.png"  "$ICONSET/icon_512x512.png"
cp "$ICON_DIR/icon_1024.png" "$ICONSET/icon_512x512@2x.png"

iconutil -c icns "$ICONSET" -o "$ICNS"
rm -rf "$(dirname "$ICONSET")"

# --- Build universal binary ---

echo "==> Building universal binary..."
export MACOSX_DEPLOYMENT_TARGET=10.13

cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

mkdir -p "$PROJECT_DIR/dist"

lipo -create \
  "$PROJECT_DIR/target/x86_64-apple-darwin/release/cobra_commander" \
  "$PROJECT_DIR/target/aarch64-apple-darwin/release/cobra_commander" \
  -output "$PROJECT_DIR/dist/cobra_commander"

# --- Assemble app bundle ---

echo "==> Assembling Cobra Commander.app..."
APP="$PROJECT_DIR/dist/Cobra Commander.app"
rm -rf "$APP"

mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

# Rename binary so macOS displays "Cobra Commander" in menus.
cp "$PROJECT_DIR/dist/cobra_commander" "$APP/Contents/MacOS/Cobra Commander"
chmod +x "$APP/Contents/MacOS/Cobra Commander"

cp "$ICNS" "$APP/Contents/Resources/CobraCommander.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>Cobra Commander</string>
    <key>CFBundleIdentifier</key>
    <string>com.generalelectrix.cobra-commander</string>
    <key>CFBundleName</key>
    <string>Cobra Commander</string>
    <key>CFBundleDisplayName</key>
    <string>Cobra Commander</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>CobraCommander</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>COBRA COMMANDER DEMANDS ACCESS TO YOUR AUDIO INPUT.</string>
    <key>NSLocalNetworkUsageDescription</key>
    <string>COBRA COMMANDER DEMANDS ACCESS TO YOUR NETWORK.</string>
</dict>
</plist>
PLIST

echo "==> Signing app bundle..."
codesign -s - --force --deep --identifier com.generalelectrix.cobra-commander "$APP"

# --- Create DMG ---

echo "==> Creating DMG..."
DMG="$PROJECT_DIR/dist/CobraCommander.dmg"
rm -f "$DMG"
create-dmg \
  --volname "Cobra Commander" \
  --window-size 600 400 \
  --icon-size 128 \
  --icon "Cobra Commander.app" 150 210 \
  --app-drop-link 450 210 \
  "$DMG" "$APP"

echo "==> Done: $DMG"
