#!/usr/bin/env bash
#
# create-dmg.sh — Wrap Cobra Commander.app into an installable DMG.
#
# Usage: ./scripts/create-dmg.sh <version>
# Example: ./scripts/create-dmg.sh 0.1.0
#
# Expects: dist/Cobra Commander.app/ (created by bundle-macos.sh)
# Output:  dist/CobraCommander-<version>-macOS.dmg

set -euo pipefail

VERSION="$1"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

APP_NAME="Cobra Commander"
DIST_DIR="$PROJECT_DIR/dist"
DMG_NAME="CobraCommander-${VERSION}-macOS.dmg"
DMG_PATH="$DIST_DIR/$DMG_NAME"
STAGING_DIR="$DIST_DIR/dmg-staging"

# Verify .app exists
if [ ! -d "$DIST_DIR/$APP_NAME.app" ]; then
    echo "Error: $DIST_DIR/$APP_NAME.app not found. Run bundle-macos.sh first." >&2
    exit 1
fi

# Clean previous DMG and staging
rm -f "$DMG_PATH"
rm -rf "$STAGING_DIR"

# Create staging directory with .app and Applications symlink
mkdir -p "$STAGING_DIR"
cp -R "$DIST_DIR/$APP_NAME.app" "$STAGING_DIR/"
ln -s /Applications "$STAGING_DIR/Applications"

# Create DMG
hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$DMG_PATH"

# Clean up staging
rm -rf "$STAGING_DIR"

echo "Created: $DMG_PATH"
