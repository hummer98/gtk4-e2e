#!/usr/bin/env bash
# Drive `bunx gtk4-e2e record` around `bun test packages/demo/scenarios/`
# to produce artifacts/demo-run.mp4. Linux X11 only (macOS / Wayland skip).
#
# Recording resolution
# --------------------
# The recorder CLI does not yet expose a `--video-size` flag (see plan §M-1).
# x11grab therefore auto-detects the resolution from the connected X display:
#   - On CI, `xvfb-run --server-args="-screen 0 1280x720x24"` pins the size
#     to 1280x720, which matches plan §2's "video_size 1280x720" intent.
#   - On a local Linux X11 host the recording size is whatever the user's
#     display is currently set to; this is acceptable for ad-hoc demos but
#     means "1280x720" is not guaranteed off-CI.
#
# Initial verification checklist (when running this script for the first time
# on a real Linux X11 host or in CI; macOS exits early with code 6):
#   - `bunx gtk4-e2e record status` (or recorder.json under runtimeDir())
#     reports a `display` field matching the active display (e.g. ":99" under
#     xvfb-run).
#   - The produced mp4 has a non-zero frame count, e.g.
#       ffprobe -v error -count_frames -select_streams v:0 \
#         -show_entries stream=nb_read_frames artifacts/demo-run.mp4
#
# Override knobs (env vars):
#   OUTPUT       output mp4 path        (default: artifacts/demo-run.mp4)
#   FPS          recorder framerate     (default: 15)
#   ARTIFACT_DIR artifact directory     (default: artifacts)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

ARTIFACT_DIR="${ARTIFACT_DIR:-artifacts}"
OUTPUT="${OUTPUT:-$ARTIFACT_DIR/demo-run.mp4}"
FPS="${FPS:-15}"

mkdir -p "$ARTIFACT_DIR"

# shellcheck disable=SC2329  # invoked via `trap cleanup EXIT` below
cleanup() {
  # Best-effort: stop the recorder if still running (e.g. bun test failed).
  if bun packages/client/src/cli.ts record status >/dev/null 2>&1; then
    bun packages/client/src/cli.ts record stop >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

# Pre-warm the cargo build so the first frames aren't a blank screen while
# `_setup.ts:spawnDemo` runs `cargo build`.
cargo build -p gtk4-e2e-demo --features e2e

bun packages/client/src/cli.ts record start --output "$OUTPUT" --fps "$FPS"

# `set +e` so we still hit `record stop` cleanly when bun test fails.
set +e
bun test packages/demo/scenarios/
TEST_EXIT=$?
set -e

bun packages/client/src/cli.ts record stop
trap - EXIT

if [ ! -s "$OUTPUT" ]; then
  echo "no recording produced at $OUTPUT" >&2
  exit 1
fi

echo "wrote $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"

exit "$TEST_EXIT"
