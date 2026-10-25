#!/usr/bin/env bash
# Compiled from scripts/amber/bg-compute.ab
# amber build scripts/amber/bg-compute.ab scripts/amber/bg-compute.sh --minify
#
# NOTE (2026-10-25): `amber build` currently fails on this source with
# "Unclosed string literal" at EOF, on the unmodified file as well as on edits,
# so the generated shell is maintained by hand until the parser is fixed. The
# arrays below must stay in sync with NAMES/ARGS in bg-compute.ab.
#
# Run detached: bash scripts/amber/bg-compute.sh &
# Or blocking:  bash scripts/amber/bg-compute.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NOTIFY="${SCRIPT_DIR}/mastodon-notify.sh"

mkdir -p .dev-tasks

relay_0="${WORMHOLE_RELAY_URL:-}"
if [ -n "${relay_0}" ]; then
    echo "bg-compute: relay=${relay_0} — Wave 3 NodeCmd::Spawn pending; running locally"
fi

NAMES=("workspace" "simd")
ARGS=(
    "--workspace"
    "-p crabcc-core --features bench --bench compress_simd"
)

# Fan-out: start all benches concurrently, exit code written to .exit sentinel
declare -a pids=()
for i in 0 1; do
    name="${NAMES[$i]}"
    log=".dev-tasks/bench-${name}.log"
    # shellcheck disable=SC2086
    (cargo bench ${ARGS[$i]} > "${log}" 2>&1; echo $? > "${log}.exit") &
    pids[$i]=$!
    printf '{"job":"%s","pid":%d}\n' "${name}" "${pids[$i]}" >> .dev-tasks/bg-jobs.jsonl
    echo "bg-compute: ${name} pid=${pids[$i]}"
done

echo "bg-compute: waiting for ${#pids[@]} jobs..."

# Barrier — block until every job finishes
wait "${pids[@]}"

# Collect outcomes
declare -a parts=()
for name in "${NAMES[@]}"; do
    code="$(cat ".dev-tasks/bench-${name}.log.exit" 2>/dev/null || echo "1")"
    if [ "${code}" = "0" ]; then
        parts+=("✓ ${name}")
    else
        parts+=("✗ ${name}")
    fi
done

summary="crabcc benches: ${parts[0]} | ${parts[1]} | ${parts[2]}"
echo "${summary}"

# Post combined summary to public timeline
if [ -n "${MASTODON_ACCESS_TOKEN:-}" ]; then
    MASTODON_VISIBILITY="${MASTODON_VISIBILITY:-public}" \
        bash "${NOTIFY}" "${summary}"
    echo "bg-compute: posted to social.crabcc.app"
else
    echo "bg-compute: MASTODON_ACCESS_TOKEN not set — skipped"
fi
