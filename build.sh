#!/bin/zsh
# Builds Suite42 apps into build/<App>.app.
#   ./build.sh                 # all apps
#   ./build.sh PDF42           # one app
#   ./build.sh PDF42 install   # and copy to /Applications
#   ./build.sh icons           # only redraw Resources/*/AppIcon.icns
set -euo pipefail
cd "$(dirname "$0")"

ALL=($(for a in PDF42 DOC42 XLS42 IMG42 PPT42 SCN42; do [[ -d Sources/$a ]] && echo $a; done))

# App icons are drawn in code (scripts/make_icon.swift) and committed; redrawn when the script changes.
make_icon() {
  local RES="Resources/$1"
  if [[ ! -f "$RES/AppIcon.icns" || scripts/make_icon.swift -nt "$RES/AppIcon.icns" || "${2:-}" == "force" ]]; then
    swift scripts/make_icon.swift "$RES" "$1"
    iconutil -c icns "$RES/AppIcon.iconset" -o "$RES/AppIcon.icns"
    rm -rf "$RES/AppIcon.iconset"
    echo "Drew $RES/AppIcon.icns"
  fi
}
if [[ "${1:-}" == "icons" ]]; then
  for a in $ALL; do make_icon $a force; done
  exit 0
fi

APPS=(${1:-})
[[ -z "${1:-}" ]] && APPS=($ALL)
INSTALL=${2:-}
OUTPUT_DIR=${SUITE42_BUILD_DIR:-build}

# Quick Look extensions hosted by each app: "<kind> <content types…>" (kind: preview | thumbnail).
# Only formats macOS itself shows as plain text or not at all.
typeset -A QL_TYPES
QL_TYPES=(
  DOC42 "net.daringfireball.markdown"
  XLS42 "org.oasis-open.opendocument.spreadsheet org.oasis-open.opendocument.spreadsheet-template public.tab-separated-values-text"
)

swift build -c release

# make_appex <App.app> <host> <kind: preview|thumbnail>
make_appex() {
  local APP=$1 HOST=$2 KIND=$3
  local BIN NAME POINT PRINCIPAL EXTRA
  if [[ $KIND == preview ]]; then
    BIN=QLPreview42; NAME="$HOST Preview"; POINT=com.apple.quicklook.preview; PRINCIPAL=PreviewProvider
    EXTRA="<key>QLIsDataBasedPreview</key><true/><key>QLSupportsSearchableItems</key><false/>"
  else
    BIN=QLThumbnail42; NAME="$HOST Thumbnail"; POINT=com.apple.quicklook.thumbnail; PRINCIPAL=ThumbnailProvider
    EXTRA="<key>QLThumbnailMinimumDimension</key><integer>0</integer>"
  fi
  local HOST_ID=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "Resources/$HOST/Info.plist")
  local VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "Resources/$HOST/Info.plist")
  local BUILD=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "Resources/$HOST/Info.plist")
  local TYPES=""
  for t in ${=QL_TYPES[$HOST]}; do TYPES+="<string>$t</string>"; done
  # Keep the extension's declared languages identical to its host app.
  local LOCALIZATIONS=$(/usr/libexec/PlistBuddy -x -c "Print :CFBundleLocalizations" "Resources/$HOST/Info.plist" | sed -n '/<array>/,/<\/array>/p')
  local X="$APP/Contents/PlugIns/$BIN.appex"
  mkdir -p "$X/Contents/MacOS" "$X/Contents/Resources"
  cp ".build/release/$BIN" "$X/Contents/MacOS/"
  cp -R ".build/release/Suite42_Kit42.bundle" "$X/Contents/Resources/"
  cat > "$X/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleLocalizations</key>$LOCALIZATIONS
	<key>CFBundleDisplayName</key><string>$NAME</string>
	<key>CFBundleExecutable</key><string>$BIN</string>
	<key>CFBundleIdentifier</key><string>$HOST_ID.$KIND</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key><string>$NAME</string>
	<key>CFBundlePackageType</key><string>XPC!</string>
	<key>CFBundleShortVersionString</key><string>$VERSION</string>
	<key>CFBundleVersion</key><string>$BUILD</string>
	<key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
	<key>LSMinimumSystemVersion</key><string>14.0</string>
	<key>NSHumanReadableCopyright</key><string>© 2026 sfzorin</string>
	<key>NSExtension</key>
	<dict>
		<key>NSExtensionAttributes</key>
		<dict><key>QLSupportedContentTypes</key><array>$TYPES</array>$EXTRA</dict>
		<key>NSExtensionPointIdentifier</key><string>$POINT</string>
		<key>NSExtensionPrincipalClass</key><string>$PRINCIPAL</string>
	</dict>
</dict>
</plist>
PLIST
  plutil -lint -s "$X/Contents/Info.plist"
  # Extensions must be sandboxed or Quick Look ignores them.
  codesign --force -s - --entitlements Resources/QuickLook/Extension.entitlements "$X"
}

for APP_NAME in $APPS; do
  RES="Resources/$APP_NAME"
  make_icon $APP_NAME

  APP="$OUTPUT_DIR/$APP_NAME.app"
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp ".build/release/$APP_NAME" "$APP/Contents/MacOS/"
  cp "$RES/Info.plist" "$APP/Contents/"
  cp "$RES/AppIcon.icns" "$APP/Contents/Resources/"
  cp -R ".build/release/Suite42_Kit42.bundle" "$APP/Contents/Resources/"
  [[ -d .build/release/Suite42_$APP_NAME.bundle ]] && cp -R ".build/release/Suite42_$APP_NAME.bundle" "$APP/Contents/Resources/"
  if [[ -n "${QL_TYPES[$APP_NAME]:-}" ]]; then
    make_appex "$APP" $APP_NAME preview
    make_appex "$APP" $APP_NAME thumbnail
  fi
  # Not --deep: that would re-sign the extensions without their sandbox entitlements.
  codesign --force -s - "$APP"
  echo "Built $APP"

  if [[ "$INSTALL" == "install" ]]; then
    rm -rf /Applications/$APP_NAME.app
    cp -R "$APP" /Applications/
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/$APP_NAME.app
    # Quick Look picks up the extensions of the registered copy.
    [[ -d /Applications/$APP_NAME.app/Contents/PlugIns ]] && pluginkit -a /Applications/$APP_NAME.app/Contents/PlugIns/*.appex 2>/dev/null || true
    echo "Installed /Applications/$APP_NAME.app"
  fi
done
