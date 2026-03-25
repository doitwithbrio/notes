#!/usr/bin/env bash
set -euo pipefail

# This script is called by tauri-action via tauriScript.
# tauri-action invokes it as: ./build-and-sign.sh build --target <target>
#
# It runs the normal `cargo tauri` build, then ad-hoc codesigns the
# resulting .app bundle so macOS doesn't report it as "damaged."

# Run the actual Tauri build (pass all arguments through)
cargo tauri "$@"

# Find and codesign the .app bundle
# Parse --target from the arguments to locate the bundle
TARGET=""
for i in "$@"; do
  case "$prev" in
    --target) TARGET="$i" ;;
  esac
  prev="$i"
done

if [ -z "$TARGET" ]; then
  echo "No --target specified, skipping codesign"
  exit 0
fi

BUNDLE_DIR="src-tauri/target/${TARGET}/release/bundle/macos"
APP_BUNDLE=$(find "$BUNDLE_DIR" -name "*.app" -maxdepth 1 2>/dev/null | head -1)

if [ -z "$APP_BUNDLE" ]; then
  echo "::warning::No .app bundle found at $BUNDLE_DIR"
  exit 0
fi

echo "Ad-hoc codesigning $APP_BUNDLE"
codesign --force --deep -s - "$APP_BUNDLE"
codesign --verify --deep --strict "$APP_BUNDLE"
echo "Codesign verified OK"

# Repackage the .app.tar.gz updater artifact with the signed bundle
APP_NAME=$(basename "$APP_BUNDLE")
TAR_GZ=$(find "$BUNDLE_DIR" -name "*.app.tar.gz" -maxdepth 1 2>/dev/null | head -1)

if [ -n "$TAR_GZ" ]; then
  echo "Repackaging $TAR_GZ with signed app"
  (cd "$BUNDLE_DIR" && tar czf "$(basename "$TAR_GZ")" "$APP_NAME")

  # Regenerate the updater .sig file
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    SIG_FILE="${TAR_GZ}.sig"
    rm -f "$SIG_FILE"
    cargo tauri signer sign --private-key "$TAURI_SIGNING_PRIVATE_KEY" "$TAR_GZ"
    echo "Updater signature regenerated"
  fi
fi

# Repackage the DMG with the signed bundle
DMG=$(find "$BUNDLE_DIR" -name "*.dmg" -maxdepth 1 2>/dev/null | head -1)
if [ -n "$DMG" ]; then
  echo "Recreating DMG with signed app"
  MOUNT_POINT=$(mktemp -d)
  hdiutil attach "$DMG" -mountpoint "$MOUNT_POINT" -quiet
  # Copy the signed app over the unsigned one in the mounted DMG
  rm -rf "$MOUNT_POINT/$APP_NAME"
  cp -R "$APP_BUNDLE" "$MOUNT_POINT/$APP_NAME"
  hdiutil detach "$MOUNT_POINT" -quiet
  echo "DMG updated with signed app"
fi

echo "Build and sign complete"
