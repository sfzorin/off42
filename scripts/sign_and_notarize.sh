#!/bin/zsh
# Signs the DMG apps with your Developer ID (hardened runtime, secure timestamp), notarizes them with Apple and
# builds the release DMG, so they open on any Mac without warnings. Requires a paid Apple Developer account.
# (The Mac App Store variant is built by scripts/build-app-store.sh instead.)
#
# One-time setup:
#   1. developer.apple.com → Certificates: create a "Developer ID Application" certificate (it appears in your
#      login Keychain).
#   2. Store notarization credentials, either an app-specific password (appleid.apple.com) …
#        xcrun notarytool store-credentials suite42 --apple-id YOU@EXAMPLE.COM --team-id TEAMID
#      … or an App Store Connect API key:
#        xcrun notarytool store-credentials suite42 --key AuthKey_XXXX.p8 --key-id XXXX --issuer ISSUER-UUID
#
# Usage:
#   DEVELOPER_ID="Developer ID Application: Your Name (TEAMID)" VERSION=1.0.0-preview.3 scripts/sign_and_notarize.sh
#       all six apps → build/dist/off42-<VERSION>-arm64.dmg (signed, notarized, stapled)
#   … scripts/sign_and_notarize.sh PDF42 DOC42     some apps only
# Options (environment):
#   NOTARY_PROFILE=suite42   keychain profile from step 2
#   STAPLE_APPS=1            also notarize and staple each app before it goes into the DMG (works offline after
#                            copying the app out of the DMG; one extra notarization per app)
#   DMG=0                    no DMG: notarized, stapled build/dist/<App>.zip per app (the previous behaviour)
#   SKIP_BUILD=1             sign the bundles already in build/ instead of running ./build.sh
set -euo pipefail
cd "$(dirname "$0")/.."
: "${DEVELOPER_ID:?Set DEVELOPER_ID to your 'Developer ID Application: …' identity}"
PROFILE=${NOTARY_PROFILE:-suite42}
APPS=(${@:-PDF42 DOC42 XLS42 IMG42 PPT42 SCN42})
MAKE_DMG=${DMG:-1}
VERSION=${VERSION:-$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" Resources/PDF42/Info.plist)}
ARCH=$(uname -m)

[[ "${SKIP_BUILD:-}" == "1" ]] || for APP in $APPS; do ./build.sh $APP; done
mkdir -p build/dist

notarize() {  # notarize <file to submit>
  xcrun notarytool submit "$1" --keychain-profile "$PROFILE" --wait
}

sign() {  # sign <path> [entitlements]
  local args=(--force --options runtime --timestamp --sign "$DEVELOPER_ID")
  [[ -n "${2:-}" ]] && args+=(--entitlements "$2")
  codesign $args "$1"
}

for APP in $APPS; do
  BUNDLE="build/$APP.app"
  echo "== $APP: signing"
  # Inside out, without --deep (which would drop the extensions' sandbox entitlements):
  # the PKCS#11 helper loads the card vendor's module, so it may load libraries signed by another team.
  for H in "$BUNDLE"/Contents/Helpers/*(N); do sign "$H" Resources/DeveloperID/P11Bridge42.entitlements; done
  for X in "$BUNDLE"/Contents/PlugIns/*.appex(N); do sign "$X" Resources/QuickLook/Extension.entitlements; done
  # The hardened runtime blocks the camera (SCN42: Continuity Camera) unless the app holds the entitlement.
  ENT="Resources/DeveloperID/$APP.entitlements"
  if [[ -f "$ENT" ]]; then sign "$BUNDLE" "$ENT"; else sign "$BUNDLE"; fi
  codesign --verify --deep --strict --verbose=2 "$BUNDLE"

  if [[ "$MAKE_DMG" != "1" || "${STAPLE_APPS:-}" == "1" ]]; then
    ZIP="build/dist/$APP.zip"
    ditto -c -k --keepParent "$BUNDLE" "$ZIP"
    echo "== $APP: notarizing (a few minutes)"
    notarize "$ZIP"
    xcrun stapler staple "$BUNDLE"
    rm "$ZIP"
    spctl --assess --type execute --verbose "$BUNDLE"
    if [[ "$MAKE_DMG" != "1" ]]; then
      ditto -c -k --keepParent "$BUNDLE" "$ZIP"
      echo "== $APP: ready → $ZIP"
    fi
  fi
done

[[ "$MAKE_DMG" == "1" ]] || exit 0

DMG_PATH="build/dist/off42-$VERSION-$ARCH.dmg"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
for APP in $APPS; do ditto "build/$APP.app" "$STAGE/$APP.app"; done
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG_PATH"
echo "== DMG: $DMG_PATH"
hdiutil create -volname "OFF42" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG_PATH"
codesign --force --timestamp --sign "$DEVELOPER_ID" "$DMG_PATH"
echo "== DMG: notarizing (a few minutes)"
notarize "$DMG_PATH"
xcrun stapler staple "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"
spctl --assess --type open --context context:primary-signature --verbose "$DMG_PATH"
shasum -a 256 "$DMG_PATH"
echo "== ready → $DMG_PATH ($(stat -f %z "$DMG_PATH") bytes)"
