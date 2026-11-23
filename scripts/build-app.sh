#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "ChooBro requires macOS." >&2
    exit 1
fi

messages=$(mktemp)
trap 'rm -f -- "$messages"' EXIT HUP INT TERM
cargo build --release --locked --bin choobro --message-format=json-render-diagnostics > "$messages"
executable=$(python3 - "$messages" <<'PYTHON'
import json
import sys

with open(sys.argv[1]) as messages:
    for line in messages:
        event = json.loads(line)
        if (event.get("reason") == "compiler-artifact"
                and event["target"]["kind"] == ["bin"]
                and event["target"]["name"] == "choobro"
                and event.get("executable")):
            print(event["executable"])
            break
    else:
        sys.exit("Cargo did not report the ChooBro executable")
PYTHON
)
bundle="dist/ChooBro.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$executable" "$bundle/Contents/MacOS/choobro.new"
mv -f "$bundle/Contents/MacOS/choobro.new" "$bundle/Contents/MacOS/choobro"
cp resources/Info.plist "$bundle/Contents/Info.plist"
version=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json, sys; print(json.load(sys.stdin)["packages"][0]["version"])')
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$bundle/Contents/Info.plist"
cp LICENSE "$bundle/Contents/Resources/LICENSE"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign - "$bundle"
echo "Built $bundle (ad-hoc signed for local development; not notarized)."
