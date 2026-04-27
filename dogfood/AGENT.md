# AI Agent Dogfooding Brief

You are running inside the `logos-scaffold` dogfood lab. The source checkout is mounted read-only at `REPO_SOURCE`. The writable dogfood root is mounted at `DOGFOOD_ROOT`, using the same absolute path that exists on the Raspberry Pi host. Keep all work and artifacts under `DOGFOOD_ROOT`.

## Mission

Dogfood the package under test and generated applications by following `DOGFOODING.md`. Use the runbook as the source of scenarios, but choose the exact commands needed for the current state. Do not edit the source checkout.

## Required Layout

Create one run directory per session:

```bash
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
WORK_ROOT="$DOGFOOD_ROOT/work/$RUN_ID"
ARTIFACT_ROOT="$DOGFOOD_ROOT/artifacts/$RUN_ID"
mkdir -p "$WORK_ROOT" "$ARTIFACT_ROOT"
rsync -a --delete --exclude target --exclude .dogfood "$REPO_SOURCE/" "$WORK_ROOT/logos-scaffold/"
cd "$WORK_ROOT/logos-scaffold"
```

Use the copied checkout as the repository under test. Build the local binary from there:

```bash
cargo build
export REPO_ROOT="$WORK_ROOT/logos-scaffold"
export SCAFFOLD_BIN="$REPO_ROOT/target/debug/logos-scaffold"
export SCRATCH_ROOT="$WORK_ROOT/generated"
mkdir -p "$SCRATCH_ROOT"
```

## Preflight

Capture host/container facts before running scenarios:

```bash
{
  date -u
  uname -a
  rustc -V
  cargo -V
  docker version
  docker run --rm --platform linux/amd64 debian:bookworm uname -m
} | tee "$ARTIFACT_ROOT/preflight.txt"
```

Expected architecture output inside this lab is `x86_64`. If nested Docker cannot run `linux/amd64`, stop and report the host setup blocker.

## Evidence Contract

Write these files:

- `summary.md`: what was tested, overall result, and highest-impact findings.
- `commands.ndjson`: one JSON object per important command with scenario id, cwd, command, exit code, duration, and short stdout/stderr excerpts.
- `findings.md`: actionable bugs or DX issues, with expected behavior, actual behavior, reproduction command, and evidence file.
- Scenario logs or raw command captures as needed under `ARTIFACT_ROOT`.

Preserve raw JSON outputs for commands such as `doctor --json`, `localnet status --json`, and deploy `--json`.

## Scenario Guidance

- Start with `E1` and project creation before long-running setup/build flows.
- Use fresh generated project directories for core scenarios.
- Keep default-template and `lez-framework` runs separate.
- Use `logos-scaffold build` for generated projects. Use direct `cargo run --bin ...` only for example runners after setup, localnet, build, and deploy prerequisites are satisfied.
- Capture contradictory localnet state before restarting or cleaning it.
- Inspect report archives with `tar -tzf` before treating them as shareable evidence.

## Cleanup

Before finishing:

```bash
"$SCAFFOLD_BIN" localnet stop || true
```

Run that from each generated project where localnet may be active. Do not delete evidence unless the human operator asks you to.
