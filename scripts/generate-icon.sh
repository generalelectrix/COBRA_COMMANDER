#!/usr/bin/env bash
#
# generate-icon.sh — Create AppIcon.icns from the 1024x1024 master PNG.
#
# Requires macOS (uses sips and iconutil).
# Input:  macos/icon-1024.png (committed to repo)
# Output: macos/AppIcon.icns
#
# Usage: ./scripts/generate-icon.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ICONSET_DIR="$PROJECT_DIR/macos/AppIcon.iconset"
OUTPUT="$PROJECT_DIR/macos/AppIcon.icns"
SOURCE_PNG="$PROJECT_DIR/macos/icon-1024.png"

if [ ! -f "$SOURCE_PNG" ]; then
    echo "Error: $SOURCE_PNG not found." >&2
    exit 1
fi

# Create .iconset with all required sizes
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

for SIZE in 16 32 128 256 512; do
    sips -z $SIZE $SIZE "$SOURCE_PNG" --out "$ICONSET_DIR/icon_${SIZE}x${SIZE}.png" > /dev/null
    DOUBLE=$((SIZE * 2))
    sips -z $DOUBLE $DOUBLE "$SOURCE_PNG" --out "$ICONSET_DIR/icon_${SIZE}x${SIZE}@2x.png" > /dev/null
done

# 512@2x is the 1024 original
cp "$SOURCE_PNG" "$ICONSET_DIR/icon_512x512@2x.png"

# Convert .iconset to .icns
iconutil -c icns "$ICONSET_DIR" -o "$OUTPUT"

# Clean up
rm -rf "$ICONSET_DIR"

echo "Created: $OUTPUT"
