#!/usr/bin/env bash
#
# build-release.sh — Build, bundle, and package Cobra Commander for macOS.
#
# Usage: ./scripts/build-release.sh [version]
# Example: ./scripts/build-release.sh 0.2.0
#
# Default version is read from Cargo.toml.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
fi

echo "Building Cobra Commander v$VERSION"

cd "$PROJECT_DIR"
cargo build --release
"$SCRIPT_DIR/generate-icon.sh"
"$SCRIPT_DIR/bundle-macos.sh" target/release/cobra_commander "$VERSION"
"$SCRIPT_DIR/create-dmg.sh" "$VERSION"
