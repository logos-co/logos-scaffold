#!/usr/bin/env bash
set -euo pipefail

: "${REPO_SOURCE:?REPO_SOURCE must be set}"
: "${DOGFOOD_ROOT:?DOGFOOD_ROOT must be set}"

RUN_ID="${RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
WORK_ROOT="${DOGFOOD_ROOT}/work/${RUN_ID}"
ARTIFACT_ROOT="${DOGFOOD_ROOT}/artifacts/${RUN_ID}"
LOG_DIR="${ARTIFACT_ROOT}/logs"
mkdir -p "${WORK_ROOT}" "${ARTIFACT_ROOT}" "${LOG_DIR}"

COMMANDS_NDJSON="${ARTIFACT_ROOT}/commands.ndjson"
: > "${COMMANDS_NDJSON}"

{
  date -u
  uname -a
  if [ -r /proc/cpuinfo ]; then
    grep -m1 -E '^(model name|Hardware|Processor|flags|Features)' /proc/cpuinfo || true
    grep -m1 -E '^(flags|Features)' /proc/cpuinfo || true
  fi
} > "${ARTIFACT_ROOT}/bootstrap-env.txt"

set +e
"${REPO_SOURCE}/dogfood/bootstrap.sh" 2>&1 | tee "${ARTIFACT_ROOT}/bootstrap.txt"
bootstrap_status=${PIPESTATUS[0]}
set -e
if [ "${bootstrap_status}" -ne 0 ]; then
  cat > "${ARTIFACT_ROOT}/summary.md" <<EOF
# Dogfood summary

Run ID: \`${RUN_ID}\`
Artifact root: \`${ARTIFACT_ROOT}\`
Executed scenario IDs: none; bootstrap failed before scenario execution.
Commands recorded: 0.

Overall result: blocked

Highest impact blockers:
- Dogfood bootstrap exited ${bootstrap_status}. See \`${ARTIFACT_ROOT}/bootstrap.txt\`.
- Bootstrap environment details: \`${ARTIFACT_ROOT}/bootstrap-env.txt\`.
EOF
  cat > "${ARTIFACT_ROOT}/findings.md" <<EOF
# Findings

## F1: Dogfood bootstrap failed before scenarios could run

Expected: dogfood bootstrap installs or validates logos-blockchain-circuits and allows E1/E2, D1-D6, and L1-L4 to execute.

Actual: bootstrap exited ${bootstrap_status}; no scenarios were executed.

Evidence: \`${ARTIFACT_ROOT}/bootstrap.txt\`.
Environment: \`${ARTIFACT_ROOT}/bootstrap-env.txt\`.
EOF
  echo "Dogfood run blocked during bootstrap: ${ARTIFACT_ROOT}" >&2
  exit "${bootstrap_status}"
fi
export LOGOS_BLOCKCHAIN_CIRCUITS="${LOGOS_BLOCKCHAIN_CIRCUITS:-${DOGFOOD_ROOT}/cache/logos-blockchain-circuits}"

json_escape() {
  python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))'
}

slugify() {
  tr -cs 'A-Za-z0-9._-' '_' | sed 's/^_//; s/_$//; s/^$/command/'
}

record_command() {
  local scenario="$1"
  local cwd="$2"
  local command="$3"
  local exit_code="$4"
  local duration="$5"
  local expected_exit_codes="$6"
  local log="$7"
  local stdout_excerpt stderr_excerpt
  stdout_excerpt="$(sed -n '/^--- stdout ---$/,/^--- stderr ---$/p' "${log}" | sed '1d;$d' | tail -c 8000 | json_escape)"
  stderr_excerpt="$(sed -n '/^--- stderr ---$/,$p' "${log}" | sed '1d' | tail -c 8000 | json_escape)"
  python3 - "$COMMANDS_NDJSON" "$scenario" "$cwd" "$command" "$exit_code" "$duration" "$expected_exit_codes" "$stdout_excerpt" "$stderr_excerpt" "$log" <<'PY'
import json, sys
path, scenario, cwd, command, exit_code, duration, expected_exit_codes, stdout_excerpt, stderr_excerpt, log = sys.argv[1:]
expected = [int(code) for code in expected_exit_codes.split(",") if code]
actual = int(exit_code)
row = {
    "scenario": scenario,
    "cwd": cwd,
    "command": command,
    "exit_code": actual,
    "expected_exit_codes": expected,
    "outcome": "expected" if actual in expected else "unexpected",
    "duration_seconds": int(duration),
    "stdout_excerpt": json.loads(stdout_excerpt),
    "stderr_excerpt": json.loads(stderr_excerpt),
    "log": log,
}
with open(path, "a", encoding="utf-8") as f:
    f.write(json.dumps(row) + "\n")
PY
}

run_cmd() {
  run_cmd_expect "$1" "$2" "0" "${@:3}"
}

run_cmd_expect() {
  local scenario="$1"
  local timeout_sec="$2"
  local expected_exit_codes="$3"
  shift 3
  local command="$*"
  local stamp slug log start end exit_code
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  slug="$(printf '%s' "${scenario}_${command}" | slugify | cut -c1-180)"
  log="${LOG_DIR}/${stamp}_${slug}.log"
  start="$(date +%s)"
  set +e
  {
    echo "[${scenario}] ${stamp}"
    echo "  cwd: ${PWD}"
    echo "  cmd: ${command}"
    echo "  expected exit: ${expected_exit_codes}"
    echo "--- stdout ---"
    timeout --preserve-status --kill-after=20s "${timeout_sec}s" bash -lc "${command}"
    exit_code=$?
    echo "--- stderr ---" >&2
  } > >(tee "${log}.stdout") 2> >(tee "${log}.stderr" >&2)
  exit_code=${exit_code:-$?}
  set -e
  end="$(date +%s)"
  {
    cat "${log}.stdout"
    cat "${log}.stderr"
    echo "[${scenario}] exit=${exit_code} duration=$((end-start))s"
  } > "${log}"
  rm -f "${log}.stdout" "${log}.stderr"
  record_command "${scenario}" "${PWD}" "${command}" "${exit_code}" "$((end-start))" "${expected_exit_codes}" "${log}"
  return 0
}

rsync -a --delete --exclude target --exclude .dogfood "${REPO_SOURCE}/" "${WORK_ROOT}/logos-scaffold/"
cd "${WORK_ROOT}/logos-scaffold"

{
  date -u
  uname -a
  rustc -V
  cargo -V
  docker version
  docker run --rm --platform linux/amd64 debian:bookworm uname -m
  echo "LOGOS_BLOCKCHAIN_CIRCUITS=${LOGOS_BLOCKCHAIN_CIRCUITS}"
} | tee "${ARTIFACT_ROOT}/preflight.txt"

run_cmd PREP 1800 "cargo build"
export REPO_ROOT="${WORK_ROOT}/logos-scaffold"
export SCAFFOLD_BIN="${REPO_ROOT}/target/debug/logos-scaffold"
export SCRATCH_ROOT="${WORK_ROOT}/generated"
mkdir -p "${SCRATCH_ROOT}"

run_cmd E1 60 '"$SCAFFOLD_BIN" --help'
run_cmd E1 60 '"$SCAFFOLD_BIN" --version'
run_cmd E1 60 '"$SCAFFOLD_BIN" help'
run_cmd_expect E1 60 "1,2" '"$SCAFFOLD_BIN" nonexistent-command'
run_cmd_expect E1 60 "1,2" '"$SCAFFOLD_BIN" build'

cd "${SCRATCH_ROOT}"
run_cmd_expect E2 120 "1,2" '"$SCAFFOLD_BIN" new dogfood-invalid-template --template nonexistent-template'
run_cmd E2 900 '"$SCAFFOLD_BIN" new dogfood-lez-explicit --template lez-framework'
run_cmd E2 900 '"$SCAFFOLD_BIN" new dogfood-vendor --vendor-deps'
run_cmd E2 900 '"$SCAFFOLD_BIN" new dogfood-cache --cache-root "$SCRATCH_ROOT/custom-cache"'

run_cmd D1 900 '"$SCAFFOLD_BIN" new dogfood-default'
run_cmd D1 900 '"$SCAFFOLD_BIN" create dogfood-default-create'
cd "${SCRATCH_ROOT}/dogfood-default"
run_cmd D1 1800 '"$SCAFFOLD_BIN" setup'
run_cmd D1 120 '"$SCAFFOLD_BIN" localnet start --timeout-sec 30'
run_cmd D1 3600 '"$SCAFFOLD_BIN" build'
run_cmd D1 900 '"$SCAFFOLD_BIN" deploy'
run_cmd D1 300 '"$SCAFFOLD_BIN" wallet topup'
run_cmd D1 120 '"$SCAFFOLD_BIN" wallet -- check-health'

run_cmd D2 120 '"$SCAFFOLD_BIN" localnet status'
run_cmd D2 120 '"$SCAFFOLD_BIN" localnet status --json | tee "$ARTIFACT_ROOT/default-localnet-status.json"'
run_cmd D2 180 '"$SCAFFOLD_BIN" doctor'
run_cmd D2 180 '"$SCAFFOLD_BIN" doctor --json | tee "$ARTIFACT_ROOT/default-doctor.json"'
run_cmd D2 120 '"$SCAFFOLD_BIN" localnet logs --tail 200'
run_cmd D2 120 '"$SCAFFOLD_BIN" localnet stop'
run_cmd D2 120 '"$SCAFFOLD_BIN" localnet status'

run_cmd D3 120 '"$SCAFFOLD_BIN" localnet start --timeout-sec 30'
run_cmd D3 900 '"$SCAFFOLD_BIN" deploy hello_world'
run_cmd D3 900 'EXAMPLE_PROGRAMS_BUILD_DIR="$PWD/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/docker"; "$SCAFFOLD_BIN" deploy --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" --json | tee "$ARTIFACT_ROOT/default-deploy-program-path.json"'

run_cmd D4 120 '"$SCAFFOLD_BIN" wallet list | tee "$ARTIFACT_ROOT/default-wallet-list.txt"'
run_cmd D4 120 '"$SCAFFOLD_BIN" wallet list --long'
run_cmd D4 120 '"$SCAFFOLD_BIN" wallet topup --dry-run'
run_cmd D4 120 '"$SCAFFOLD_BIN" wallet -- account list'

run_cmd D5 180 '"$SCAFFOLD_BIN" report --tail 200'
run_cmd D5 180 'mkdir -p artifacts; "$SCAFFOLD_BIN" report --out "$PWD/artifacts/support-report.tar.gz"'
run_cmd D5 120 'tar -tzf "$PWD/artifacts/support-report.tar.gz" | sort | tee "$ARTIFACT_ROOT/default-report-custom-listing.txt"'

run_cmd D6 120 '"$SCAFFOLD_BIN" wallet -- account new public | tee "$ARTIFACT_ROOT/default-d6-account-new.txt"'
run_cmd D6 900 'ACCOUNT_ID="$(awk "/Generated new account/{print \$6}" "$ARTIFACT_ROOT/default-d6-account-new.txt" | sed "s#Public/##")"; export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"; cargo run --bin run_hello_world -- "$ACCOUNT_ID"'
run_cmd D6 900 'ACCOUNT_ID="$(awk "/Generated new account/{print \$6}" "$ARTIFACT_ROOT/default-d6-account-new.txt" | sed "s#Public/##")"; "$SCAFFOLD_BIN" wallet -- account get --account-id "$ACCOUNT_ID"'
run_cmd D6 120 '"$SCAFFOLD_BIN" localnet stop'

cd "${SCRATCH_ROOT}"
run_cmd L1 900 '"$SCAFFOLD_BIN" new dogfood-lez --template lez-framework'
cd "${SCRATCH_ROOT}/dogfood-lez"
run_cmd L1 1800 '"$SCAFFOLD_BIN" setup'
run_cmd L1 120 '"$SCAFFOLD_BIN" localnet start --timeout-sec 30'
run_cmd L1 180 '"$SCAFFOLD_BIN" doctor'
run_cmd L1 3600 '"$SCAFFOLD_BIN" build'
run_cmd L2 1800 '"$SCAFFOLD_BIN" build idl --timeout-sec 1200'
run_cmd L2 120 'find idl -maxdepth 1 -type f -name "*.json" | sort | tee "$ARTIFACT_ROOT/lez-idl-files.txt"'
run_cmd L3 1800 '"$SCAFFOLD_BIN" build client --timeout-sec 1200'
run_cmd L3 120 'find src/generated -type f | sort | tee "$ARTIFACT_ROOT/lez-generated-files.txt"'
run_cmd L4 900 '"$SCAFFOLD_BIN" deploy'
run_cmd L4 120 '"$SCAFFOLD_BIN" wallet -- account new public | tee "$ARTIFACT_ROOT/lez-account-new.txt"'
run_cmd L4 900 'ACCOUNT_ID="$(awk "/Generated new account/{print \$6}" "$ARTIFACT_ROOT/lez-account-new.txt" | sed "s#Public/##")"; export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"; cargo run --bin run_lez_counter -- init --to "$ACCOUNT_ID"'
run_cmd L4 900 'ACCOUNT_ID="$(awk "/Generated new account/{print \$6}" "$ARTIFACT_ROOT/lez-account-new.txt" | sed "s#Public/##")"; export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"; cargo run --bin run_lez_counter -- increment --counter "$ACCOUNT_ID" --authority "$ACCOUNT_ID" --amount 5'
run_cmd L4 120 '"$SCAFFOLD_BIN" localnet stop'

python3 - "$COMMANDS_NDJSON" "$ARTIFACT_ROOT" <<'PY'
import json, sys
commands_path, artifact_root = sys.argv[1:]
rows = [json.loads(line) for line in open(commands_path, encoding="utf-8") if line.strip()]
failed = [r for r in rows if r["outcome"] != "expected"]
scenarios = sorted(set(r["scenario"] for r in rows))
summary = [
    "# Dogfood summary",
    "",
    f"Run ID: `{artifact_root.rsplit('/', 1)[-1]}`",
    f"Artifact root: `{artifact_root}`",
    f"Executed scenario IDs: {', '.join(scenarios)}.",
    f"Commands recorded: {len(rows)}.",
    "",
    "Overall result: " + ("pass" if not failed else "partial pass"),
    "",
    "Highest impact blockers:",
]
if failed:
    for row in failed[:12]:
        summary.append(f"- `{row['scenario']}` `{row['command']}` exited {row['exit_code']} (log: `{row['log']}`).")
else:
    summary.append("- None.")
open(f"{artifact_root}/summary.md", "w", encoding="utf-8").write("\n".join(summary) + "\n")

findings = ["# Findings", ""]
for i, row in enumerate(failed, 1):
    findings += [
        f"## F{i}: Scenario {row['scenario']} command failed",
        "",
        f"Expected: `{row['command']}` succeeds or fails only when the runbook marks it as a negative-path check.",
        "",
        f"Actual: exited with code {row['exit_code']}.",
        "",
        f"Evidence: `{row['log']}`.",
        "",
    ]
if not failed:
    findings.append("No actionable failures recorded.")
open(f"{artifact_root}/findings.md", "w", encoding="utf-8").write("\n".join(findings) + "\n")
PY

echo "Dogfood run complete: ${ARTIFACT_ROOT}"
