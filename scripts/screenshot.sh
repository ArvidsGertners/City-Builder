#!/usr/bin/env bash
# Renders a screenshot suite (ADR-0058) into docs/screenshots/<suite>/ with metrics.json.
# Usage: scripts/screenshot.sh <suite> [out_dir]
# Linux without a GPU: uses Xvfb + lavapipe. Set CB_SCREENSHOT_NATIVE=1 to use the real
# display/GPU instead (also the only mode on macOS and Windows).
# CB_PROFILE selects the cargo profile (default: dev).
set -euo pipefail
cd "$(dirname "$0")/.."

suite="${1:?usage: scripts/screenshot.sh <suite> [out_dir]}"
out="${2:-docs/screenshots/$suite}"
profile="${CB_PROFILE:-dev}"
target_dir="$profile"
[ "$profile" = "dev" ] && target_dir="debug"

cargo build -p cb_game --profile "$profile"
mkdir -p "$out"
userdata="$(mktemp -d)"
trap 'rm -rf "$userdata"' EXIT
export CB_USERDATA="$userdata"

bin="target/$target_dir/cb_game"
if [ "${CB_SCREENSHOT_NATIVE:-0}" = "1" ] || [ "$(uname)" != "Linux" ]; then
    "$bin" --screenshot-suite "$suite" --out "$out"
else
    export VK_ICD_FILENAMES="${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.json}"
    export WGPU_BACKEND="${WGPU_BACKEND:-vulkan}"
    xvfb-run -a -s "-screen 0 1920x1080x24" "$bin" --screenshot-suite "$suite" --out "$out"
fi
echo "screenshots: $out"
