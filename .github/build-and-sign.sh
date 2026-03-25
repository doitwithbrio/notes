#!/usr/bin/env bash
set -euo pipefail

# Called by tauri-action via tauriScript.
# tauri-action invokes: ./build-and-sign.sh build --target <target>
#
# 1. Runs npx tauri build
# 2. Ad-hoc codesigns the .app bundle
# 3. Repackages .app.tar.gz with the signed bundle
# 4. Regenerates the updater .sig file

# Use npx tauri (installed by tauri-action as @tauri-apps/cli)
npx tauri "$@"

# Parse --target from arguments
TARGET=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--target" ]; then
    TARGET="$arg"
  fi
  prev="$arg"
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
  TAR_BASENAME=$(basename "$TAR_GZ")
  (cd "$BUNDLE_DIR" && rm -f "$TAR_BASENAME" && tar czf "$TAR_BASENAME" "$APP_NAME")

  # Regenerate the updater .sig file
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    SIG_FILE="${TAR_GZ}.sig"
    rm -f "$SIG_FILE"
    npx tauri signer sign --private-key "$TAURI_SIGNING_PRIVATE_KEY" "$TAR_GZ"
    echo "Updater signature regenerated"
  fi
fi

echo "Build and sign complete"
