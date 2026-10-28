#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "ChooBro requires macOS." >&2
    exit 1
fi

cargo build --release --locked
bundle="dist/ChooBro.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/choobro "$bundle/Contents/MacOS/choobro.new"
mv -f "$bundle/Contents/MacOS/choobro.new" "$bundle/Contents/MacOS/choobro"
cp resources/Info.plist "$bundle/Contents/Info.plist"
cp LICENSE "$bundle/Contents/Resources/LICENSE"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign - "$bundle"
echo "Built $bundle (ad-hoc signed for local development; not notarized)."
