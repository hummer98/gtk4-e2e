#!/usr/bin/env bash
# Generates the Linux+xvfb baseline PNG for
# packages/demo/scenarios/visual-regression.spec.ts inside Docker so the
# committed bytes match the CI environment exactly (ubuntu:24.04 +
# xvfb 1280x720x24). See plan §"判断 2" / README §"Visual regression baseline".
#
# Usage:
#   bash packages/demo/scripts/gen-visual-baseline.sh
#
# Requires Docker daemon (Docker Desktop / dockerd) running on the host.
# First run takes ~5-10 minutes (apt install + cargo build); subsequent
# runs reuse the bind-mounted target/ cache.
#
# Locale is pinned to C.UTF-8 to keep font rendering deterministic across
# repeated runs (plan §R3 non-determinism check).

set -euxo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"

if ! docker info >/dev/null 2>&1; then
  echo "Docker daemon is not reachable. Start Docker Desktop / dockerd and retry." >&2
  exit 1
fi

docker run --rm \
  -v "$REPO_ROOT":/repo \
  -w /repo \
  -e GTK4_E2E_UPDATE_BASELINE=1 \
  ubuntu:24.04 \
  bash -c '
    set -euxo pipefail
    export DEBIAN_FRONTEND=noninteractive
    export LC_ALL=C.UTF-8
    export LANG=C.UTF-8
    apt-get update
    apt-get install -y --no-install-recommends \
      libgtk-4-dev libglib2.0-dev pkg-config build-essential \
      xvfb dbus-x11 curl ca-certificates unzip git
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    . "$HOME/.cargo/env"
    curl -fsSL https://bun.sh/install | bash
    export PATH="$HOME/.bun/bin:$PATH"
    cd /repo
    bun install --frozen-lockfile
    cargo build -p gtk4-e2e-demo --features e2e
    Xvfb :99 -screen 0 1280x720x24 -nolisten tcp &
    XVFB_PID=$!
    export DISPLAY=:99
    sleep 1
    set +e
    bun test packages/demo/scenarios/visual-regression.spec.ts
    rc=$?
    set -e
    kill "$XVFB_PID" 2>/dev/null || true
    wait "$XVFB_PID" 2>/dev/null || true
    exit "$rc"
  '

echo
echo "Generated baseline. Inspect:"
echo "  git status packages/demo/scenarios/__screenshots__/"
