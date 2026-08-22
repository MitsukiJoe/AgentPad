#!/bin/sh
set -eu

profile="${1:-debug}"
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

case "$profile" in
  debug)
    cargo build --manifest-path "$root/Cargo.toml" -p agentpad
    ;;
  release)
    cargo build --manifest-path "$root/Cargo.toml" -p agentpad --release
    ;;
  *)
    echo "usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

app="$root/target/$profile/AgentPad.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/macos/AgentPad.icns" "$app/Contents/Resources/AgentPad.icns"
cp "$root/target/$profile/agentpad" "$app/Contents/MacOS/agentpad"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${AGENTPAD_VERSION:-0.1.0}" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${AGENTPAD_BUILD_NUMBER:-1}" "$app/Contents/Info.plist"
/usr/bin/xattr -cr "$app"
/usr/bin/codesign --force --deep --sign "${AGENTPAD_CODESIGN_IDENTITY:-AgentPad Local Development}" --identifier app.agentpad "$app"
echo "$app"
