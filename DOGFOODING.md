# `logos-scaffold` dogfooding Scenarios

This document is the canonical dogfooding runbook for `logos-scaffold`.
Use it to evaluate the latest repository state, not as a dated findings report.
Earlier one-off dogfood notes are historical context only; future runs should start here.

> Maintenance note: update this document whenever first-class commands, templates, supported workflows, or major user-facing behaviors are added, removed, or materially changed. If the product surface changes and this runbook does not, the runbook is wrong.

## Purpose and Audience

- Dogfooders: use this as a repeatable checklist when validating the latest scaffold DX.
- Contributors: use this document to decide which scenarios must be rerun for a given change.

This guide is intentionally scenario-oriented:

- It defines what to exercise.
- It defines what success looks like.
- It calls out the failures and caveats that are worth recording.
- It does not replace generated project READMEs or CLI help text.

## Usage Model

The recommended dogfooding pattern is:

1. Build the local scaffold binary from the repository under test.
2. Create fresh generated projects in a scratch workspace outside the repo.
3. Run project-level scenarios from inside the generated project root.
4. Capture command, cwd, exit code, and a short output excerpt for each scenario.
5. If the behavior differs from this runbook, update the runbook when the difference is intentional and file a bug when it is not.

For repo dogfooding, prefer the freshly built local binary over an already-installed global binary.

```bash
export REPO_ROOT=/absolute/path/to/logos-scaffold
export SCRATCH_ROOT=/absolute/path/to/dogfood-runs
cd "$REPO_ROOT"
cargo build
export SCAFFOLD_BIN="$REPO_ROOT/target/debug/logos-scaffold"
mkdir -p "$SCRATCH_ROOT"
```

You may replace `"$SCAFFOLD_BIN"` with `logos-scaffold` when the install path itself is part of what you are validating.

## Execution Contexts

| Context | Purpose | Typical commands |
| --- | --- | --- |
| Repo root | Build the latest CLI, inspect docs, validate help/version output, verify out-of-project errors | `cargo build`, `"$SCAFFOLD_BIN" --help`, `"$SCAFFOLD_BIN" --version`, `"$SCAFFOLD_BIN" build` (expect error) |
| Scratch workspace | Create fresh generated projects without polluting the repo; test advanced creation flags | `"$SCAFFOLD_BIN" new dogfood-default`, `"$SCAFFOLD_BIN" new ... --template ...` |
| Generated project root | Execute scaffold workflows and example runners against a fresh project | `setup`, `localnet`, `build`, `deploy`, `wallet`, `doctor`, `report`, `cargo run --bin run_*` |

Do not run project-scoped commands from the repository root unless the scenario is explicitly checking the "outside project" error path.

## Shared Preconditions

- Unix-like environment with `git`, `rustc`, `cargo`, `lsof`, `ps`, and `kill`.
- Docker or Podman available for guest builds.
- No conflicting listener on the scaffold localnet port before `localnet start`.
- Network access available for setup/build flows that fetch dependencies.
- Optional but supported: `LOGOS_SCAFFOLD_WALLET_PASSWORD` when validating password override behavior.

## Scenario Index

| ID | Template | Level | Goal | Command surface |
| --- | --- | --- | --- | --- |
| D1 | `default` | Core | Fresh project creation and first-success bootstrap | `new`, `create`, `setup`, `localnet start`, `build`, `deploy`, `wallet topup`, `wallet -- check-health` |
| D2 | `default` | Core | Localnet lifecycle visibility and doctor checks | `localnet status`, `localnet logs`, `localnet stop`, `doctor`, JSON variants |
| D3 | `default` | Advanced | Deploy path variations and machine-readable single-program submission | `deploy [program-name]`, `deploy --program-path`, `deploy --program-path --json` |
| D4 | `default` | Core | Wallet management, default-address behavior, and passthrough UX | `wallet list`, `wallet default set`, `wallet topup --dry-run`, `wallet topup`, `wallet -- ...` |
| D5 | `default` | Advanced | Diagnostics bundle and support artifact hygiene | `report`, `report --out`, `report --tail` |
| D6 | `default` | Core | Example runner interaction and account state verification | `cargo run --bin run_hello_world`, `cargo run --bin run_hello_world_with_move_function`, `wallet -- account get` |
| L1 | `lez-framework` | Core | Fresh LEZ project bootstrap to ready state | `new --template lez-framework`, `setup`, `localnet start`, `doctor`, `build` |
| L2 | `lez-framework` | Core | LEZ IDL regeneration | `build idl` |
| L3 | `lez-framework` | Advanced | LEZ client generation from current IDL | `build client` |
| L4 | `lez-framework` | Core | LEZ deploy and counter interaction | `deploy`, `cargo run --bin run_lez_counter` |
| E1 | N/A | Core | CLI discoverability and error quality | `--help`, `help`, `--version`, unknown commands, out-of-project errors |
| E2 | N/A | Advanced | Project creation with advanced flags and invalid inputs | `new --template`, `new --vendor-deps`, `new --cache-root` |

## Standing Validation Notes

- Project context matters. Many scaffold commands are meant to be run only inside a generated project root. Running them elsewhere should produce a clear error, not silent misbehavior.
- Localnet readiness, listener ownership, and wallet connectivity are high-value validation points. Record contradictions instead of smoothing over them.
- Machine-readable paths matter for tooling. Preserve `--json` outputs when a scenario includes them.
- `report` is sanitized on a best-effort basis, not on an absolute guarantee. Always inspect the archive before sharing it.
- When wallet behavior depends on an omitted address, verify whether the project default wallet was seeded and persisted as expected.
- Example runner programs (`cargo run --bin run_*`) are the final proof that the scaffold pipeline works end-to-end. A successful deploy means nothing if the runner cannot interact with the deployed program.

## D1. Default Template Bootstrap and First Success

### Goal

Validate that the default template can be scaffolded from the latest repo and reach the documented first-success path.

### Preconditions

- `cargo build` completed at the repo root.
- `"$SCAFFOLD_BIN"` points to the freshly built binary.
- Scratch workspace exists and is writable.

### Commands / Actions

From the scratch workspace:

```bash
cd "$SCRATCH_ROOT"
"$SCAFFOLD_BIN" new dogfood-default
"$SCAFFOLD_BIN" create dogfood-default-create
cd dogfood-default
"$SCAFFOLD_BIN" setup
"$SCAFFOLD_BIN" localnet start
"$SCAFFOLD_BIN" build
"$SCAFFOLD_BIN" deploy
"$SCAFFOLD_BIN" wallet topup
"$SCAFFOLD_BIN" wallet -- check-health
```

Use `new` for the main runnable project and `create` as the lightweight alias-parity check in a separate directory. Both commands also accept `--template`, `--vendor-deps`, `--lez-path`, and `--cache-root`, but this scenario uses defaults only. See E2 for advanced flag coverage.

### Expected Success Signals

- Project creation succeeds and prints the destination path, pinned LEZ commit, and cache root.
- `setup` completes and either seeds the default wallet or reports that a default wallet is already configured.
- `localnet start` reports a ready localnet rather than only a spawned PID.
- `build` exits successfully after preparing the project workspace.
- `deploy` prints a submission summary with zero failures when built binaries are present.
- `wallet topup` succeeds without an explicit address because the project default wallet was seeded during setup.
- `wallet -- check-health` succeeds against the running localnet.

### Failure Signals / Common Pitfalls

- Running `setup`, `build`, `deploy`, or wallet commands outside the generated project root should fail with a project-scoped message.
- A foreign listener or stale state on the localnet port is a real dogfooding finding; capture `localnet status`, not just the final error.
- If `wallet topup` without an address says no destination is configured, record that as a regression in default-wallet seeding or persistence.
- If `deploy` fails due to missing binaries after a successful `build`, capture the exact missing path.

### Evidence to Capture

- Scaffold creation output for both `new` and `create`.
- `setup`, `localnet start`, `build`, `deploy`, and wallet command excerpts.
- The generated project path and the exact binary path used for the run.

### Execution Notes

- Use fresh directories per run. Do not reuse an old generated project unless the scenario explicitly targets upgrade or persistence behavior.
- Keep the alias check isolated so a failure in `create` does not contaminate the primary bootstrap project.

## D2. Default Template Operational Health: Localnet and Doctor

### Goal

Validate that localnet lifecycle commands and doctor diagnostics provide usable human and machine-readable state.

### Preconditions

- A default-template project exists.
- `setup` has already completed for that project.

### Commands / Actions

From the generated project root:

```bash
"$SCAFFOLD_BIN" localnet status
"$SCAFFOLD_BIN" localnet status --json
"$SCAFFOLD_BIN" doctor
"$SCAFFOLD_BIN" doctor --json
"$SCAFFOLD_BIN" localnet logs --tail 200
"$SCAFFOLD_BIN" localnet stop
"$SCAFFOLD_BIN" localnet status
```

If the scenario begins with localnet stopped, run `"$SCAFFOLD_BIN" localnet start` first and capture both the started and stopped states.

### Expected Success Signals

- Human-readable `localnet status` clearly reports tracked PID, listener state, ownership, and readiness.
- `localnet status --json` returns parseable JSON with at least `tracked_pid`, `listener_present`, `ownership`, and `ready`.
- `doctor` returns actionable next steps rather than only raw failures.
- `doctor --json` returns parseable JSON with at least `status`, `summary`, `checks`, and `next_steps`.
- `localnet logs --tail 200` returns useful recent log lines when logs exist.
- `localnet stop` succeeds cleanly and subsequent status reflects the stopped state.

### Failure Signals / Common Pitfalls

- Contradictions between tracked PID, listener ownership, and readiness are high-value findings.
- Empty or unhelpful logs after a failed startup are worth recording.
- If `doctor` omits next steps or machine-readable output becomes malformed, treat that as a DX regression.

### Evidence to Capture

- Human-readable and JSON output for both `localnet status` and `doctor`.
- A short `localnet logs` excerpt.
- Stop behavior and the post-stop status output.

### Execution Notes

- Preserve raw JSON output exactly.
- If state is contradictory, do not silently restart localnet before capturing the failing state.

## D3. Default Template Deploy Variants and JSON Output

### Goal

Validate targeted deployment flows, including the machine-readable single-program submission path via `--program-path`.

### Preconditions

- Default-template project has already completed `build`.
- Localnet is reachable.
- Guest binaries exist under the generated project's `target/riscv-guest/.../release` directory.

### Commands / Actions

From the generated project root:

```bash
export EXAMPLE_PROGRAMS_BUILD_DIR="$PWD/target/riscv-guest/example_program_deployment_methods/example_program_deployment_programs/riscv32im-risc0-zkvm-elf/release"
"$SCAFFOLD_BIN" deploy hello_world
"$SCAFFOLD_BIN" deploy --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin"
"$SCAFFOLD_BIN" deploy --program-path "$EXAMPLE_PROGRAMS_BUILD_DIR/hello_world.bin" --json
"$SCAFFOLD_BIN" deploy nonexistent_program
```

Use a known default-template program name such as `hello_world`. If the generated project exposes a different set of programs in `methods/guest/src/bin`, record the discovered list.

`--json` only produces structured JSON output when combined with `--program-path`. On the discovery-based path (`deploy` or `deploy <name>`), the `--json` flag is accepted but silently ignored. This scenario validates that distinction.

### Expected Success Signals

- `deploy hello_world` reports `OK  hello_world submitted` and ends with a human-readable success summary.
- `deploy --program-path ... --json` prints a parseable JSON object with at least `status`, `program`, and `tx` fields.
- `deploy --program-path ...` without `--json` prints a human-readable `OK` line with the binary path.
- `deploy nonexistent_program` fails with an error listing the available discovered programs.

### Failure Signals / Common Pitfalls

- If `deploy hello_world --json` starts producing JSON output (instead of the normal human-readable summary), record that as a behavior change worth verifying.
- If localnet is unreachable, deploy should fail with a sequencer-unavailable hint instead of a vague wallet error.
- Unknown program names should report the available discovered programs.
- Missing binaries should point back to `logos-scaffold build`.

### Evidence to Capture

- One successful human-readable deploy excerpt from the discovery path.
- One successful JSON deploy output from the `--program-path` path.
- The error output for an unknown program name.
- Any failure-path excerpt for unreachable sequencer or missing binary when intentionally probed.

### Execution Notes

- Keep the `--program-path --json` examples separate from discovery-based deploys. Only `--program-path` produces JSON.
- When recording a custom `--program-path`, preserve the absolute path used in the run log.

## D4. Default Template Wallet Workflows and Passthrough

### Goal

Validate wallet-focused scaffold behavior beyond the basic bootstrap path.

### Preconditions

- Default-template project exists.
- Setup completed successfully.
- Localnet is running if you are validating non-dry-run topup or passthrough health checks.

### Commands / Actions

From the generated project root:

```bash
"$SCAFFOLD_BIN" wallet list
"$SCAFFOLD_BIN" wallet list --long
"$SCAFFOLD_BIN" wallet default set Public/<account-id>
"$SCAFFOLD_BIN" wallet topup --dry-run
"$SCAFFOLD_BIN" wallet topup
"$SCAFFOLD_BIN" wallet -- account list
"$SCAFFOLD_BIN" wallet -- check-health
```

Use a real address from `wallet list` when explicitly validating `wallet default set`.

### Expected Success Signals

- `wallet list` and `wallet list --long` proxy wallet account enumeration from the project-scoped wallet home.
- `wallet default set` accepts either positional address or `--address` and persists the normalized project default.
- `wallet topup --dry-run` renders the underlying faucet claim command instead of mutating state.
- `wallet topup` without an explicit address uses the saved default wallet.
- `wallet -- ...` preserves the project wallet environment while forwarding the raw wallet command.

Optional: validate `LOGOS_SCAFFOLD_WALLET_PASSWORD` override behavior by setting the env var to a non-default value and observing whether wallet commands honor it.

```bash
LOGOS_SCAFFOLD_WALLET_PASSWORD="custom-pw" "$SCAFFOLD_BIN" wallet topup --dry-run
```

### Failure Signals / Common Pitfalls

- Invalid addresses should be rejected with an "Accepted formats" hint.
- If both positional address and `--address` are supplied together, that is a user error and should remain clearly reported.
- Connectivity failures during topup should mention localnet/sequencer reachability rather than only raw wallet output.
- Passthrough flows require the literal `--`; if the CLI starts accepting or mangling passthrough without it, record that change.

### Evidence to Capture

- `wallet list` output with account identifiers redacted only if needed for sharing.
- `wallet topup --dry-run` output showing the rendered command.
- One successful passthrough example, ideally `wallet -- check-health` or `wallet -- account list`.
- If `LOGOS_SCAFFOLD_WALLET_PASSWORD` override was tested, the dry-run output showing the password was or was not forwarded.

### Execution Notes

- Do not let the shell consume the passthrough separator. Record the exact argv form you used.
- If you redact account IDs for public sharing, keep the unredacted originals in a local evidence log so repeated runs stay traceable.

## D5. Default Template Diagnostics Bundle

### Goal

Validate that scaffold support artifacts can be collected and inspected safely.

### Preconditions

- Default-template project exists.
- The project has enough state to make the report meaningful, ideally after setup and at least one localnet or build action.

### Commands / Actions

From the generated project root:

```bash
"$SCAFFOLD_BIN" report
"$SCAFFOLD_BIN" report --tail 200
"$SCAFFOLD_BIN" report --out "$PWD/artifacts/support-report.tar.gz"
```

Inspect the produced archive before sharing it:

```bash
find .scaffold/reports -maxdepth 1 -name '*.tar.gz' -print | sort
REPORT_ARCHIVE="$(find .scaffold/reports -maxdepth 1 -name '*.tar.gz' | sort | tail -n 1)"
tar -tzf "$REPORT_ARCHIVE" | sort
tar -tzf "$PWD/artifacts/support-report.tar.gz" | sort
```

### Expected Success Signals

- `report` prints a completion message, archive path, and a warning to inspect files before sharing.
- The default output lands under `.scaffold/reports/`.
- A custom `--out` path is honored.
- The archive contains support files such as `README.txt`, `manifest.json`, `diagnostics/doctor.json`, `diagnostics/localnet-status.json`, and `summaries/build-evidence.json`.

### Failure Signals / Common Pitfalls

- If raw wallet files under `.scaffold/wallet/` appear in the archive, treat that as a severe regression.
- If absolute local paths leak without scrubbing in human-facing report files, record it.
- If the archive is produced but the warning about manual inspection disappears, record it.

### Evidence to Capture

- Report completion output.
- Archive path(s).
- A short file listing from the tarball.

### Execution Notes

- Never attach the archive to an external system without first listing its contents.
- Keep the tar listing with the run evidence so redaction regressions can be compared across releases.

## D6. Default Template Example Runner Interaction

### Goal

Validate that deployed programs can actually be invoked via the generated example runner binaries and that account state changes are observable.

D1 validates the scaffold pipeline up to deploy and wallet health. This scenario validates the final step: running programs against the localnet and confirming observable state mutations.

### Preconditions

- Default-template project exists with D1 completed (setup, build, deploy done).
- Localnet is running and `wallet -- check-health` succeeds.
- At least one public account exists. If not, create one:

```bash
"$SCAFFOLD_BIN" wallet -- account new public
```

Capture the account ID from the output (format: `Public/<base58>`). Use the base58 portion as `<account-id>` below.

### Commands / Actions

From the generated project root:

```bash
export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"
cargo run --bin run_hello_world -- <account-id>
"$SCAFFOLD_BIN" wallet -- account get --account-id <account-id>
cargo run --bin run_hello_world_with_move_function -- write-public <account-id> "dogfood-test-message"
"$SCAFFOLD_BIN" wallet -- account get --account-id <account-id>
```

The first runner (`run_hello_world`) submits a basic public transaction. The second (`run_hello_world_with_move_function write-public`) writes a custom greeting string to the account, producing an observable `data_b64` field change.

### Expected Success Signals

- Both runners print `submitted transaction: status=... tx_hash=...` on success.
- Both runners print a `verification hint:` line pointing to `wallet account get`.
- After `run_hello_world_with_move_function write-public`, `wallet account get` shows account data containing the encoded greeting string.
- Runner exit code is 0.

### Failure Signals / Common Pitfalls

- If a runner exits 0 but the account remains `Uninitialized`, the transaction may have been submitted without effect. Record both the runner output and the account state.
- Panic output from a runner (e.g., `unwrap()` on wallet/sequencer errors) instead of a structured error is worth recording.
- Invalid account ID format (not base58) should produce a clear parse error from the runner, not a panic.
- If localnet is down, runners should fail with a connection-refused error. Capture the exact error text.

### Evidence to Capture

- Runner output including `status` and `tx_hash` for at least one successful run.
- `wallet account get` output showing account state after interaction.
- The exact account ID used (for traceability across repeated runs).

### Execution Notes

- `NSSA_WALLET_HOME_DIR` must be set for runners that initialize `WalletCore::from_env()`. The scaffold wallet commands set this automatically, but direct `cargo run` does not.
- Create a fresh public account for this scenario rather than reusing accounts from other scenarios. This avoids confusion about pre-existing state.
- If additional runners are available (e.g., `run_hello_world_private`, `run_hello_world_through_tail_call`), exercising them is valuable but not required for this scenario.

## L1. LEZ Template Bootstrap

### Goal

Validate that the LEZ template scaffolds and reaches a ready-to-build state.

### Preconditions

- Latest scaffold binary has been built from the repo root.
- Scratch workspace exists.

### Commands / Actions

From the scratch workspace:

```bash
cd "$SCRATCH_ROOT"
"$SCAFFOLD_BIN" new dogfood-lez --template lez-framework
cd dogfood-lez
ls -d idl crates/lez-client-gen methods/guest/src/bin src/bin
"$SCAFFOLD_BIN" setup
"$SCAFFOLD_BIN" localnet start
"$SCAFFOLD_BIN" doctor
"$SCAFFOLD_BIN" build
```

The `ls` step verifies that LEZ-specific directories were scaffolded before proceeding with the build pipeline.

### Expected Success Signals

- Project creation succeeds with the LEZ template.
- The generated project contains `idl/`, `crates/lez-client-gen/`, `methods/guest/src/bin/lez_counter.rs`, and `src/bin/run_lez_counter.rs`.
- `setup`, `localnet start`, and `doctor` behave the same way they do for the default template.
- `build` succeeds for the LEZ project workspace and also runs IDL generation and client generation automatically.

### Failure Signals / Common Pitfalls

- If the generated project is missing LEZ-specific paths such as `idl/`, `crates/lez-client-gen/`, or `methods/guest/src/bin/lez_counter.rs`, record that immediately.
- If LEZ bootstrap behavior diverges from the default template in setup/localnet/doctor flows, capture the difference explicitly.
- If `build` does not automatically trigger IDL + client generation for the LEZ template, record that as a regression.

### Evidence to Capture

- LEZ project creation output.
- Directory listing showing LEZ-specific scaffolded paths.
- `setup`, `localnet start`, `doctor`, and `build` excerpts.

### Execution Notes

- Keep LEZ runs separate from default-template runs. The template-specific directories and follow-up commands are part of the validation.

## L2. LEZ IDL Regeneration

### Goal

Validate that LEZ projects can regenerate IDL from the current project source.

### Preconditions

- LEZ project exists.
- The LEZ project build environment is working.

### Commands / Actions

From the LEZ project root:

```bash
"$SCAFFOLD_BIN" build idl
find idl -maxdepth 1 -type f -name '*.json' | sort
```

### Expected Success Signals

- `build idl` writes one or more JSON files under `idl/`.
- Command output includes explicit `Wrote IDL ...` lines.
- The regenerated files are valid JSON and match the current program surface.

### Failure Signals / Common Pitfalls

- If the command prints that IDL build is being skipped due to framework kind, the scenario is running in the wrong project.
- Missing IDL marker output or empty IDL generation is a real regression for the LEZ template.

### Evidence to Capture

- `build idl` output.
- Listing of generated files under `idl/`.
- If relevant, a diff between pre-existing and regenerated IDL.

### Execution Notes

- Preserve the raw `Wrote IDL ...` lines. They make it much easier to diagnose partial-generation failures.

## L3. LEZ Client Generation

### Goal

Validate that LEZ client bindings can be regenerated from the current IDL set.

### Preconditions

- LEZ project exists.
- `build idl` has been run successfully, either directly or via `build client`.

### Commands / Actions

From the LEZ project root:

```bash
"$SCAFFOLD_BIN" build client
find src/generated -type f | sort
```

### Expected Success Signals

- `build client` reports that it is regenerating IDL before generating client code.
- Client artifacts are written under `src/generated`.
- The generated files reflect the current contents of `idl/`.

### Failure Signals / Common Pitfalls

- If `build client` does not refresh IDL first, record that behavior change.
- Missing `src/generated` output or missing generator crate paths are LEZ-specific regressions.

### Evidence to Capture

- `build client` output.
- Listing of files under `src/generated`.
- Any diff in generated client code when the scenario is rerun after a program change.

### Execution Notes

- Treat generated client output as part of the scenario evidence, not as disposable noise.
- When the generator fails, capture the exact manifest path and working directory that were used.

## L4. LEZ Template Deploy and Counter Interaction

### Goal

Validate that the LEZ counter program can be deployed and that the generated runner binary can invoke `init` and `increment` subcommands against the running localnet.

### Preconditions

- LEZ project exists with L1 completed (setup, build, localnet running).
- `wallet -- check-health` succeeds.
- At least one public account exists. If not:

```bash
"$SCAFFOLD_BIN" wallet -- account new public
```

### Commands / Actions

From the LEZ project root:

```bash
"$SCAFFOLD_BIN" deploy
export NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet"
cargo run --bin run_lez_counter -- init --to <account-id>
cargo run --bin run_lez_counter -- increment --counter <account-id> --authority <account-id> --amount 5
```

### Expected Success Signals

- `deploy` submits the `lez_counter` program and prints a success summary.
- `run_lez_counter init` prints confirmation that the counter was initialized at the target account.
- `run_lez_counter increment` prints confirmation of the increment operation.

Note: as of this writing, the LEZ counter runner contains `TODO` placeholders for actual transaction submission. If the runner only prints diagnostic messages without submitting transactions, record that as the current state. When transaction submission is implemented, update this scenario with account-state verification steps matching D6.

### Failure Signals / Common Pitfalls

- If `deploy` cannot find `lez_counter` in the discovered program list, record the actual discovered list.
- If the runner panics on wallet initialization, `NSSA_WALLET_HOME_DIR` may not be set.
- If the runner accepts the subcommand but does nothing (due to TODO stubs), record the output and note the gap.

### Evidence to Capture

- `deploy` output for the LEZ project.
- `run_lez_counter init` and `increment` output.
- Whether the runner actually submitted transactions or only printed placeholder messages.

### Execution Notes

- `NSSA_WALLET_HOME_DIR` must be set for the runner. Scaffold wallet commands set this automatically, but direct `cargo run` does not.
- Keep LEZ interaction evidence separate from default-template interaction evidence.

## E1. CLI Discoverability and Error Quality

### Goal

Validate that the scaffold CLI provides consistent, non-destructive help and version output, useful error messages for unknown commands, and clear project-context errors when commands are run outside a generated project.

### Preconditions

- Latest scaffold binary has been built from the repo root.
- A scratch workspace exists (for verifying that help flags do not create files).

### Commands / Actions

From the repo root:

```bash
"$SCAFFOLD_BIN" --help
"$SCAFFOLD_BIN" --version
"$SCAFFOLD_BIN" help
"$SCAFFOLD_BIN" nonexistent-command
"$SCAFFOLD_BIN" build
"$SCAFFOLD_BIN" deploy
"$SCAFFOLD_BIN" doctor
"$SCAFFOLD_BIN" localnet status
"$SCAFFOLD_BIN" wallet list
```

From the scratch workspace (verify help flags do not mutate the filesystem):

```bash
cd "$SCRATCH_ROOT"
ls -la before_help_test > /dev/null 2>&1 || true
"$SCAFFOLD_BIN" create --help
"$SCAFFOLD_BIN" new --help
ls -la
```

Check that no new directories were created by the `--help` invocations.

### Expected Success Signals

- `--help` prints a usage summary listing all top-level commands.
- `--version` prints the version string and exits.
- `help` prints the same top-level usage summary as `--help` and exits successfully.
- `nonexistent-command` fails with an error and directs the user to `--help` or an equivalent corrective hint.
- `build`, `deploy`, `doctor`, `localnet status`, and `wallet list` run from outside a project fail with a message like `Not a logos-scaffold project ... Run logos-scaffold create <name>`.
- `create --help` and `new --help` do not create directories or files in the current working directory.

### Failure Signals / Common Pitfalls

- If `create --help` or `new --help` creates a directory named `--help`, that is a significant UX regression. Record it and the exact argv used.
- If project-context errors are missing or unhelpful (e.g., a raw file-not-found instead of a scaffold-specific message), record the exact output.
- If some subcommands support `--help` and others do not, document the inconsistency.

### Evidence to Capture

- `--help` output.
- `--version` output.
- Error output for unknown command and out-of-project commands.
- Directory listing before and after `create --help` / `new --help` to confirm no side effects.

### Execution Notes

- Run the `create --help` test in an isolated temporary directory so any accidental file creation does not pollute the scratch workspace.
- Do not interpret missing `--help` support on a subcommand as a blocker. Record it as a finding and move on.

## E2. Project Creation with Advanced Flags

### Goal

Validate that `create`/`new` handle the `--template`, `--vendor-deps`, `--lez-path` (legacy alias: `--lssa-path`), and `--cache-root` flags correctly, including error cases for invalid inputs.

### Preconditions

- Latest scaffold binary has been built from the repo root.
- Scratch workspace exists and is writable.

### Commands / Actions

From the scratch workspace:

```bash
cd "$SCRATCH_ROOT"
"$SCAFFOLD_BIN" new dogfood-invalid-template --template nonexistent-template
"$SCAFFOLD_BIN" new dogfood-lez-explicit --template lez-framework
ls -d dogfood-lez-explicit/idl dogfood-lez-explicit/crates/lez-client-gen
"$SCAFFOLD_BIN" new dogfood-vendor --vendor-deps
"$SCAFFOLD_BIN" new dogfood-cache --cache-root "$SCRATCH_ROOT/custom-cache"
```

### Expected Success Signals

- Invalid `--template` name fails with a clear error listing the available templates (`default`, `lez-framework`).
- `--template lez-framework` creates a project with LEZ-specific structure (same as L1).
- `--vendor-deps` is accepted without error and creates a project that vendors the pinned LEZ repo under `.scaffold/repos/lez`.
- `--cache-root` is honored and scaffold uses the specified directory for cache operations.

### Failure Signals / Common Pitfalls

- If an invalid template name silently falls back to `default`, record that as a regression.
- If `--vendor-deps` or `--cache-root` are silently ignored or produce an error, record the exact output.
- If `--lez-path` is tested and the path does not exist, verify the error message points to the bad path.

### Evidence to Capture

- Error output for invalid `--template`.
- Creation output for `--template lez-framework` with directory listing.
- Creation output for `--vendor-deps` and `--cache-root` if tested.

### Execution Notes

- Clean up the generated projects after this scenario to avoid consuming disk space with multiple scaffolded projects.
- The `--lez-path` flag is optional to test here because it requires a real LEZ checkout. Only probe it if one is available.

## Minimum Rerun Guidance for Future Changes

- Changes to onboarding, project creation, setup, localnet, or build flows: rerun `D1`, `D2`, and `D6`.
- Changes to deploy behavior or deploy output formatting: rerun `D3` and `D6`.
- Changes to wallet flows or wallet-related defaults: rerun `D4`.
- Changes to diagnostics, report contents, or redaction logic: rerun `D5`.
- Changes to example runner binaries or template `src/bin/*` code: rerun `D6`.
- Changes to LEZ template scaffolding or generated outputs: rerun `L1`, `L2`, `L3`, and `L4`.
- Changes to CLI argument parsing, help text, or error messages: rerun `E1`.
- Changes to `create`/`new` flags or template selection logic: rerun `E2`.

When in doubt, rerun more scenarios rather than fewer.
