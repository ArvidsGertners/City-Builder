#!/usr/bin/env bash
# Fails if the headless simulation depends on rendering, windowing, UI or audio crates
# (CLAUDE.md "Simulation never depends on rendering", ADR-0060 CI job `sim-isolation`).
set -euo pipefail
cd "$(dirname "$0")/.."

forbidden='^(bevy_render|bevy_pbr|bevy_ui|bevy_winit|bevy_egui|bevy_audio|wgpu|winit) '
tree="$(cargo tree -p cb_sim -e normal --prefix none --format '{p}')"
if hits="$(grep -E "$forbidden" <<<"$tree" | sort -u)" && [ -n "$hits" ]; then
    echo "cb_sim depends on forbidden crates:" >&2
    echo "$hits" >&2
    exit 1
fi
echo "sim-isolation: ok ($(wc -l <<<"$tree") dependency lines checked)"
