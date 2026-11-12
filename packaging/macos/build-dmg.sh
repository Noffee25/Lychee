#!/usr/bin/env bash
# Build the macOS disk image, on a Mac:
#
#   packaging/macos/build-dmg.sh VERSION OUTDIR BINARY [BINARY…]
#
# Puts the release binary in Lychee.app and makes OUTDIR/Lychee-VERSION-macos.dmg, which holds the
# app and a shortcut to Applications to drag it onto. Several binaries (the Apple silicon and Intel
# builds) are joined into one universal binary.
#
# The app is signed ad hoc, not with an Apple Developer ID, so macOS asks people to confirm the
# first launch (see packaging/release-notes.md).
set -euo pipefail

version="${1:?version}"
out="${2:?output folder}"
shift 2
if [ "$#" -eq 0 ]; then
    echo "usage: $0 VERSION OUTDIR BINARY [BINARY...]" >&2
    exit 2
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

app="$work/Lychee/Lychee.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
if [ "$#" -gt 1 ]; then
    lipo -create -output "$app/Contents/MacOS/lychee" "$@"
else
    cp "$1" "$app/Contents/MacOS/lychee"
fi
chmod 755 "$app/Contents/MacOS/lychee"
lipo -info "$app/Contents/MacOS/lychee"

# CFBundleVersion only allows numbers and dots, so "0.2.0-beta" becomes "0.2.0" there.
sed -e "s/@VERSION@/$version/g" -e "s/@BUILD@/${version%%-*}/g" "$root/packaging/macos/Info.plist" \
    >"$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
cp "$root/assets/macos/lychee.icns" "$app/Contents/Resources/lychee.icns"
cp "$root/LICENSE" "$app/Contents/Resources/LICENSE.txt"

# Apple silicon only runs signed code, and the signature also seals Info.plist and the icon.
codesign --force --sign - "$app"
codesign --verify --strict --verbose=1 "$app"

ln -s /Applications "$work/Lychee/Applications"
dmg="$out/Lychee-$version-macos.dmg"
# HFS+: left to itself, hdiutil gives the image the source's file system, APFS on current macOS, and
# those images can come out too small for the app ("No space left on device", actions/runner-images#4288).
hdiutil create -volname Lychee -srcfolder "$work/Lychee" -fs HFS+ -format UDZO -ov "$dmg"
echo "built $dmg"
