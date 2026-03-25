#!/usr/bin/env bash
#
# build-release.sh — Build, bundle, and package Cobra Commander for macOS.
#
# Usage: ./scripts/build-release.sh [--single-arch] [version]
#
# By default, builds a universal binary (ARM64 + x86_64) via cross-compilation
# and lipo. Use --single-arch to build only for the host architecture (faster
# for dev iteration).
#
# Default version is read from Cargo.toml.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

SINGLE_ARCH=false
VERSION=""

for arg in "$@"; do
    case "$arg" in
        --single-arch) SINGLE_ARCH=true ;;
        *) VERSION="$arg" ;;
    esac
done

if [ -z "$VERSION" ]; then
    VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
fi

echo "Building Cobra Commander v$VERSION"
cd "$PROJECT_DIR"

if [ "$SINGLE_ARCH" = true ]; then
    echo "Building for host architecture only"
    cargo build --release
    BINARY="target/release/cobra_commander"
else
    echo "Building universal binary (aarch64 + x86_64)"

    # Ensure both targets are installed
    rustup target add aarch64-apple-darwin x86_64-apple-darwin 2>/dev/null || true

    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin

    mkdir -p target/universal-release
    lipo -create \
        target/aarch64-apple-darwin/release/cobra_commander \
        target/x86_64-apple-darwin/release/cobra_commander \
        -output target/universal-release/cobra_commander

    echo "Universal binary: $(lipo -info target/universal-release/cobra_commander)"
    BINARY="target/universal-release/cobra_commander"
fi

"$SCRIPT_DIR/generate-icon.sh"
"$SCRIPT_DIR/bundle-macos.sh" "$BINARY" "$VERSION"
"$SCRIPT_DIR/create-dmg.sh" "$VERSION"
