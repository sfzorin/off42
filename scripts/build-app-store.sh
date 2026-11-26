#!/bin/zsh
# Builds the sandboxed Mac App Store variant of the six apps into build/app-store/<App>.app (and .pkg).
# The DMG build (./build.sh without SUITE42_APP_STORE) is not affected: this variant has its own Swift build
# folder (.build/app-store), its own output folder and the compile condition APP_STORE.
#
#   scripts/build-app-store.sh                 # all apps, ad-hoc signed: a local check of the sandbox setup
#   scripts/build-app-store.sh PDF42 SCN42     # some apps
#
# Signing for App Store Connect (see appstore/README.md):
#   APP_STORE_TEAM_ID=ABCDE12345                                     # Team ID (App Group prefix, application id)
#   APP_STORE_IDENTITY="3rd Party Mac Developer Application: Name (ABCDE12345)"   # or "Apple Distribution: …"
#   APP_STORE_INSTALLER_IDENTITY="3rd Party Mac Developer Installer: Name (ABCDE12345)"
#   APP_STORE_PROFILES=dir     # <App>.provisionprofile, <App>.preview.provisionprofile, <App>.thumbnail.provisionprofile
# Optional:
#   APP_STORE_BUNDLE_PREFIX=cc.off42   # bundle ids cc.off42.pdf42 …; must match the ids registered in the account
#   APP_STORE_BUILD_NUMBER=7           # CFBundleVersion; App Store Connect needs a higher number for every upload
#   APP_STORE_VALIDATE=1 / APP_STORE_UPLOAD=1 with APP_STORE_API_KEY + APP_STORE_API_ISSUER (App Store Connect API
#   key; the .p8 file in ~/.appstoreconnect/private_keys/): runs `xcrun altool --validate-app` / `--upload-app`.
set -euo pipefail
cd "$(dirname "$0")/.."

ALL=(PDF42 DOC42 XLS42 IMG42 PPT42 SCN42)
APPS=(${@:-$ALL})
OUT=build/app-store
TEAM=${APP_STORE_TEAM_ID:-}
PREFIX=${APP_STORE_BUNDLE_PREFIX:-cc.off42}
GROUP="${TEAM:-TEAMID}.cc.off42"
IDENTITY=${APP_STORE_IDENTITY:--}
INSTALLER=${APP_STORE_INSTALLER_IDENTITY:-}
if [[ "$IDENTITY" != "-" && -z "$TEAM" ]]; then
  echo "APP_STORE_TEAM_ID is required with APP_STORE_IDENTITY (App Group and application identifier)." >&2; exit 1
fi

# One build for all requested apps; build.sh compiles with -D APP_STORE and leaves out P11Bridge42.
for APP_NAME in $APPS; do SUITE42_APP_STORE=1 SUITE42_BUILD_DIR=$OUT ./build.sh $APP_NAME; done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
PB=/usr/libexec/PlistBuddy

# entitlements <source> <bundle id> <output>: Team ID filled in; application/team identifiers for a real signature.
entitlements() {
  sed "s/\$(TEAM_ID)/${TEAM:-TEAMID}/g" "$1" > "$3"
  if [[ -n "$TEAM" ]]; then
    $PB -c "Add :com.apple.application-identifier string $TEAM.$2" "$3"
    $PB -c "Add :com.apple.developer.team-identifier string $TEAM" "$3"
  fi
  plutil -lint -s "$3"
}

profile() {  # profile <bundle> <name>
  if [[ -n "${APP_STORE_PROFILES:-}" ]]; then
    local P="$APP_STORE_PROFILES/$2.provisionprofile"
    [[ -f "$P" ]] || { echo "Missing provisioning profile $P" >&2; exit 1; }
    cp "$P" "$1/Contents/embedded.provisionprofile"
  fi
}

for APP_NAME in $APPS; do
  APP="$OUT/$APP_NAME.app"
  LOWER=${APP_NAME:l}
  ID="$PREFIX.$LOWER"
  PLIST="$APP/Contents/Info.plist"
  [[ -d "$APP/Contents/Helpers" ]] && { echo "$APP contains helpers; the App Store variant must not" >&2; exit 1; }

  $PB -c "Set :CFBundleIdentifier $ID" "$PLIST"
  $PB -c "Add :Suite42AppGroup string $GROUP" "$PLIST"
  # Export compliance: encryption only through Apple's frameworks (TLS, PDFKit passwords, Security/CryptoKit
  # signatures); see appstore/README.md. The owner confirms this in App Store Connect.
  $PB -c "Add :ITSAppUsesNonExemptEncryption bool false" "$PLIST"
  $PB -c "Print :LSApplicationCategoryType" "$PLIST" >/dev/null 2>&1 \
    || $PB -c "Add :LSApplicationCategoryType string public.app-category.productivity" "$PLIST"
  [[ -n "${APP_STORE_BUILD_NUMBER:-}" ]] && $PB -c "Set :CFBundleVersion $APP_STORE_BUILD_NUMBER" "$PLIST"
  cp "Resources/$APP_NAME/PrivacyInfo.xcprivacy" "$APP/Contents/Resources/"
  plutil -lint -s "$PLIST"

  # Quick Look extensions: own bundle ids, App Store entitlements (no temporary exceptions), signed first.
  for X in "$APP"/Contents/PlugIns/*.appex(N); do
    KIND=${${X:t:r}#QL}; KIND=${${KIND%42}:l}            # QLPreview42 → preview
    XPLIST="$X/Contents/Info.plist"
    $PB -c "Set :CFBundleIdentifier $ID.$KIND" "$XPLIST"
    [[ -n "${APP_STORE_BUILD_NUMBER:-}" ]] && $PB -c "Set :CFBundleVersion $APP_STORE_BUILD_NUMBER" "$XPLIST"
    profile "$X" "$APP_NAME.$KIND"
    entitlements Resources/QuickLook/AppStore.entitlements "$ID.$KIND" "$WORK/$APP_NAME.$KIND.entitlements"
    codesign --force --sign "$IDENTITY" --entitlements "$WORK/$APP_NAME.$KIND.entitlements" "$X"
  done

  profile "$APP" "$APP_NAME"
  entitlements "Resources/$APP_NAME/AppStore.entitlements" "$ID" "$WORK/$APP_NAME.entitlements"
  # Not --deep: that would re-sign the extensions with the app's entitlements.
  codesign --force --sign "$IDENTITY" --entitlements "$WORK/$APP_NAME.entitlements" "$APP"
  codesign --verify --strict --verbose=1 "$APP"
  echo "Signed $APP ($ID, ${IDENTITY/#-/ad-hoc})"

  PKG="$OUT/$APP_NAME.pkg"
  if [[ -n "$INSTALLER" ]]; then
    productbuild --component "$APP" /Applications --sign "$INSTALLER" "$PKG"
  else
    productbuild --component "$APP" /Applications "$PKG"   # unsigned: structure check only, not uploadable
  fi
  echo "Built $PKG"

  if [[ "${APP_STORE_VALIDATE:-}" == "1" || "${APP_STORE_UPLOAD:-}" == "1" ]]; then
    : "${APP_STORE_API_KEY:?Set APP_STORE_API_KEY (App Store Connect API key id)}"
    : "${APP_STORE_API_ISSUER:?Set APP_STORE_API_ISSUER (App Store Connect issuer id)}"
    [[ -n "$INSTALLER" ]] || { echo "Validation needs a signed .pkg (APP_STORE_INSTALLER_IDENTITY)" >&2; exit 1; }
    xcrun altool --validate-app -f "$PKG" -t macos --apiKey "$APP_STORE_API_KEY" --apiIssuer "$APP_STORE_API_ISSUER"
    if [[ "${APP_STORE_UPLOAD:-}" == "1" ]]; then
      xcrun altool --upload-app -f "$PKG" -t macos --apiKey "$APP_STORE_API_KEY" --apiIssuer "$APP_STORE_API_ISSUER"
    fi
  fi
done
