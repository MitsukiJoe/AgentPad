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
mkdir -p "$app/Contents/MacOS"
cp "$root/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/target/$profile/agentpad" "$app/Contents/MacOS/agentpad"
/usr/bin/codesign --force --deep --sign "${AGENTPAD_CODESIGN_IDENTITY:-AgentPad Local Development}" --identifier app.agentpad "$app"
echo "$app"
