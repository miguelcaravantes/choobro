#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo "Preview DMGs must be built on an Apple Silicon Mac." >&2
    exit 1
fi

sh scripts/build-app.sh
bundle="dist/ChooBro.app"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$bundle/Contents/Info.plist")
name="ChooBro-$version-macos-arm64.dmg"
stage=$(mktemp -d "$PWD/dist/dmg.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT HUP INT TERM

test "$(lipo -archs "$bundle/Contents/MacOS/choobro")" = arm64
codesign --verify --deep --strict "$bundle"
ditto "$bundle" "$stage/ChooBro.app"
ln -s /Applications "$stage/Applications"
cp resources/INSTALL.txt "$stage/INSTALL.txt"
hdiutil create -volname ChooBro -srcfolder "$stage" -format UDZO -ov "dist/$name"
hdiutil verify "dist/$name"

cd dist
shasum -a 256 "$name" > "$name.sha256"
shasum -a 256 -c "$name.sha256"
echo "Created dist/$name and its SHA-256 checksum (not notarized)."
