#!/usr/bin/env bash
#
# bundle-macos.sh — Assemble a macOS .app bundle from a compiled binary.
#
# Usage: ./scripts/bundle-macos.sh <binary-path> <version>
# Example: ./scripts/bundle-macos.sh target/release/cobra_commander 0.1.0
#
# Output: dist/Cobra Commander.app/

set -euo pipefail

BINARY="$1"
VERSION="$2"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

APP_NAME="Cobra Commander"
APP_DIR="$PROJECT_DIR/dist/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

# Clean previous bundle
rm -rf "$APP_DIR"

# Create bundle structure
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

# Copy binary
cp "$BINARY" "$MACOS_DIR/cobra_commander"
chmod +x "$MACOS_DIR/cobra_commander"

# Stamp version into Info.plist and copy
sed "s/__VERSION__/$VERSION/g" "$PROJECT_DIR/macos/Info.plist" > "$CONTENTS_DIR/Info.plist"

# Copy icon if it exists
if [ -f "$PROJECT_DIR/macos/AppIcon.icns" ]; then
    cp "$PROJECT_DIR/macos/AppIcon.icns" "$RESOURCES_DIR/AppIcon.icns"
fi

# Ad-hoc code sign
codesign --force --deep --sign - "$APP_DIR"

echo "Bundled: $APP_DIR (version $VERSION)"
