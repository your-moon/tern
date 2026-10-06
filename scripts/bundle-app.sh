#!/usr/bin/env bash
# Builds a release tern and assembles target/release/bundle/tern.app.
#
#   scripts/bundle-app.sh
#
# Signing: always ad-hoc so the bundle runs on this Mac. With TERN_SIGN_ID (a "Developer ID
# Application: ..." identity) it is signed with the hardened runtime instead; with
# TERN_NOTARY_PROFILE too (a `xcrun notarytool store-credentials` profile) it is notarised and
# stapled. Without them those steps are skipped, with a message.
#
# Plain shell rather than cargo-bundle: the bundle is one binary, one plist and one icon (fonts,
# icons and themes are compiled in with include_bytes!), and a script adds signing and
# notarisation, which cargo-bundle does not do.
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE_ID="mn.tern.app"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }

cargo build --release -p tern

app=target/release/bundle/tern.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/tern "$app/Contents/MacOS/tern"
cp packaging/macos/tern.icns "$app/Contents/Resources/tern.icns"

# The oldest macOS the binary loads on is whatever rustc linked it for; read it back instead of
# guessing (gpui itself supports 10.15, rustc's arm64 floor is 11.0).
minos=$(otool -l "$app/Contents/MacOS/tern" | awk '/LC_BUILD_VERSION/ {f=1} f && /minos/ {print $2; exit}')
minos=${minos:-11.0}

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>tern</string>
  <key>CFBundleIconFile</key><string>tern</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>tern</string>
  <key>CFBundleDisplayName</key><string>tern</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key><string>${minos}</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist"

if [ -n "${TERN_SIGN_ID:-}" ]; then
  codesign --force --deep --options runtime --timestamp -s "$TERN_SIGN_ID" "$app"
  echo "signed with: $TERN_SIGN_ID"
else
  codesign --force --deep -s - "$app"
  echo "ad-hoc signed (set TERN_SIGN_ID to sign with a Developer ID; skipping that)"
fi

if [ -n "${TERN_SIGN_ID:-}" ] && [ -n "${TERN_NOTARY_PROFILE:-}" ]; then
  zip=$(mktemp -d)/tern-notarize.zip
  ditto -c -k --keepParent "$app" "$zip"
  xcrun notarytool submit "$zip" --keychain-profile "$TERN_NOTARY_PROFILE" --wait
  xcrun stapler staple "$app"
  echo "notarised and stapled"
else
  echo "not notarised (needs TERN_SIGN_ID and TERN_NOTARY_PROFILE; skipping that)"
fi

codesign --verify --deep --strict "$app"
echo "built $app ($version)"
