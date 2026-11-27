#!/bin/sh
# Checks a built OFF42.app (CI): privacy manifest and Info.plist keys present, and that the binary references none
# of Apple's required-reason APIs that PrivacyInfo.xcprivacy does not declare (it declares none), nor any networking
# or tracking API.
#   sh ios/scripts/check-app-bundle.sh build/OFF42.xcarchive/Products/Applications/OFF42.app
set -eu
app=$1
binary="$app/OFF42"
status=0

test -f "$app/PrivacyInfo.xcprivacy" || { echo "PrivacyInfo.xcprivacy missing"; status=1; }
plutil -lint "$app/PrivacyInfo.xcprivacy"
for key in CFBundleDocumentTypes UTImportedTypeDeclarations LSSupportsOpeningDocumentsInPlace ITSAppUsesNonExemptEncryption; do
  plutil -extract "$key" raw -o - "$app/Info.plist" > /dev/null 2>&1 || plutil -extract "$key" xml1 -o - "$app/Info.plist" > /dev/null \
    || { echo "Info.plist: $key missing"; status=1; }
done
echo "Bundle id: $(plutil -extract CFBundleIdentifier raw -o - "$app/Info.plist")"
echo "Localizations: $(ls -d "$app"/*.lproj | xargs -n1 basename | tr '\n' ' ')"

# C functions of the required-reason categories; each category must be declared in the privacy manifest.
symbols=$(xcrun nm -u "$binary" | sed 's/^ *//')
declared() { grep -q "$1" "$app/PrivacyInfo.xcprivacy"; }
check_category() { # category, symbol pattern
  found=$(echo "$symbols" | grep -E "^_($2)(\\\$.*)?$" || true)
  if [ -n "$found" ]; then
    echo "$1: $(echo $found)"
    declared "$1" || { echo "  not declared in PrivacyInfo.xcprivacy"; status=1; }
  elif declared "$1"; then
    echo "$1 declared but no such symbol is referenced (remove it from PrivacyInfo.xcprivacy?)"
  fi
}
check_category NSPrivacyAccessedAPICategoryFileTimestamp 'stat|fstat|fstatat|lstat|getattrlist|getattrlistbulk|fgetattrlist|getattrlistat'
check_category NSPrivacyAccessedAPICategorySystemBootTime 'mach_absolute_time'
check_category NSPrivacyAccessedAPICategoryDiskSpace 'statfs|fstatfs|statvfs|fstatvfs'
# Foundation/UIKit APIs of those categories, user defaults and keyboards, by name.
api=$(strings -a "$binary" | grep -E 'NSUserDefaults|UserDefaults|systemUptime|activeInputModes|volumeAvailableCapacity|creationDate|contentModificationDate|fileModificationDate' | sort -u || true)
if [ -n "$api" ]; then
  echo "Required-reason Foundation APIs named in the binary:"
  echo "$api"
  status=1
fi
# No networking: the app makes no connections (WKWebView loads only the in-memory page, see WebPage).
net=$(xcrun nm -u "$binary" | sed 's/^ *//' | grep -E '^_(socket|connect|getaddrinfo|CFSocket|nw_connection|SCNetworkReachability)' || true)
if [ -n "$net" ] || strings -a "$binary" | grep -qE 'NSURLSession|URLSessionDataTask'; then
  echo "Networking APIs referenced: $net"
  status=1
fi
[ $status -eq 0 ] && echo "App bundle checks passed"
exit $status
