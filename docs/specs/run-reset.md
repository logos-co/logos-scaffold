# Spec: `lgs run --reset-localnet`

## Assumptions

Surfacing before any code:

1. New CLI flags `--reset-localnet` / `--no-reset-localnet` on `run`,
   mutually exclusive (clap `conflicts_with`) — symmetric with the
   existing `--restart-localnet` / `--no-restart-localnet` pair.
2. New `[run].reset_localnet: bool` in `scaffold.toml` (default `false`).
3. CLI flag overrides config; config overrides default — matches the
   existing precedence rule for `restart_localnet`.
4. Reset and restart are **orthogonal at the input layer**. They can
   independently be set via CLI or config. When `reset_localnet=true`,
   `cmd_run` calls `cmd_localnet_reset` (which itself does
   stop → wipe → start → verify); the value of `restart_localnet`
   becomes a no-op in this branch because reset *already* includes a
   restart. This is an implication of what reset does, not a
   precedence rule. No warning is printed; there is no real conflict.
5. The reset performs a *full* wipe: rocksdb (`<lez>/rocksdb`), wallet
   (`<project>/<wallet_home_dir>`), wallet state file, localnet state
   file. Wallet topup at step 4 re-funds the freshly-wiped wallet, which
   is exactly what the iteration loop wants.
6. `run` calls `cmd_localnet_reset` directly with `reset_wallet=true` and
   `verify_timeout_sec=30` hardcoded. No subprocess exec, no third CLI
   flag for the timeout.
7. Cold-start works without changes: `cmd_localnet_stop` already returns
   Ok when nothing is running; `reset_cleanup` deletes only-if-exists.
   Verified by reading the code, not just assumed.
8. Step counter stays at the current shape (5 without hooks, 6 with) —
   reset *replaces* the "Ensuring localnet" step, it does not add one.
9. No interactive confirmation prompt. The flag is opt-in and
   destructive only of dev-state; gating it behind a y/n hurts the
   primary use case (tight iteration). If wallet wipe ever becomes
   regrettable, escalate then.

→ Push back on any of these now.

## Objective

The primary `lgs run` use case is a **fast iterate-deploy-interact
loop**: edit a program, re-run, exercise it via post-deploy hooks. The
inner loop should stay tight — re-running should not impose a
block-production wait if it doesn't have to.

Reset is a *secondary* mode for the cases where stale on-chain state
gets in the way: a counter that has drifted, accumulated test pollution,
or a "starting from a clean slate" reproducer. Today these users chain
`lgs localnet reset --reset-wallet && lgs run` — two commands, two
block-production waits, two chances to forget the `--reset-wallet`.

Reset stays **opt-in**, exposed as `lgs run --reset-localnet` (one-shot)
or `[run].reset_localnet = true` (project-wide default for repos where
clean-slate iteration is the norm).

### Why reset is not the default

Program IDs in LEZ are content-addressed: `ProgramId = risc0
image_id(elf)` (`nssa/src/program.rs:32-35`). On deploy, an existing
ID returns `NssaError::ProgramAlreadyExists`
(`nssa/src/validated_state_diff.rs:408-410`). Two consequences for the
default decision:

- **No silent-wrong-instance footgun.** A program edit produces a new
  ID; the new IDL points hooks at the new program. Unchanged programs
  retain their old ID and the deploy attempt errors loudly, not
  silently.
- **The legitimate "wipe everything" reasons are per-scenario** (drifted
  counter, polluted test state, repro starting clean) — not per-edit.
  Forcing a 30-second block-production wait on every iterate-deploy
  cycle is not what the loop wants.

### Deploy idempotence: already satisfied by submission semantics

Verified during implementation by reading the sequencer source:

- `send_transaction`
  (`sequencer/service/src/service.rs:46-83`) only runs
  `transaction_stateless_check`. It does not validate program
  existence; it pushes the tx to the mempool and returns Ok.
- `ProgramAlreadyExists`
  (`nssa/src/validated_state_diff.rs:408-410`) is raised *during block
  validation*, not at submission. The wallet has no signal back from
  block validation.

Consequence: `wallet deploy-program` exits 0 in both the success case
(new program registered) and the duplicate case (tx silently dropped
during block validation). `cmd_deploy` therefore does not bail when N-1
programs hit `ProgramAlreadyExists` — the pipeline is already
idempotent at the exit-code level. No deploy-side changes are required
before shipping this spec.

There is a residual silent-failure mode: if a deploy is *legitimately*
broken (e.g. malformed bytecode), it fails block validation the same
way and the scaffold reports success. That's a separate observability
gap, not a blocker for the reset feature.

## Tech Stack

- Rust 2021 edition (existing crate)
- clap derive macros for CLI parsing
- `anyhow::Result` for error propagation
- Hand-rolled TOML parser/serializer in `src/config.rs` (no `toml`
  crate dependency added)

## Commands

```bash
cargo build                                    # Debug build
cargo check                                    # Type check
cargo test --all-targets                       # Run all tests
cargo fmt --check                              # Format check (CI gate)
cargo run --bin logos-scaffold -- run \
    --reset-localnet                           # Manual smoke test
```

## Project Structure

Touched files only:

```
src/cli.rs                 → Add --reset-localnet flag pair to RunArgs
src/commands/run.rs        → Branch on effective_reset; call cmd_localnet_reset
src/config.rs              → Parse + serialize [run].reset_localnet
src/model.rs               → Add reset_localnet: bool to RunConfig
tests/cli.rs               → Integration tests for the new flag
README.md                  → Document the flag in the run section
```

No new modules. No changes to the localnet command — `cmd_localnet_reset`
is already public to the crate (`pub(crate)`).

## Code Style

The existing precedence pattern, applied to the new field:

```rust
let effective_reset = reset_localnet.unwrap_or(project.config.run.reset_localnet);
let effective_restart = restart_localnet.unwrap_or(project.config.run.restart_localnet);

// Reset and restart are orthogonal at the input layer; when reset is
// requested, restart's value is a no-op because cmd_localnet_reset
// already includes a stop+start. Not a conflict; no warning.
if effective_reset {
    println!("[3/{total_steps}] Resetting localnet (wipes sequencer + wallet)...");
    let lez = PathBuf::from(&project.config.lez.path);
    let state_path = project.root.join(".scaffold/state/localnet.state");
    let log_path = project.root.join(".scaffold/logs/sequencer.log");
    let localnet_addr = format!("127.0.0.1:{}", project.config.localnet.port);
    cmd_localnet_reset(
        &project,
        &lez,
        &state_path,
        &log_path,
        &localnet_addr,
        true,  // reset_wallet
        30,    // verify_timeout_sec
    )?;
} else {
    println!("[3/{total_steps}] Ensuring localnet...");
    ensure_localnet(&project, effective_restart)?;
}
```

Naming follows existing conventions: `effective_*` for the
post-precedence value, `cmd_*` for top-level handlers, snake_case
fields in TOML.

## Testing Strategy

Three layers, mirroring the existing `run` test pattern:

1. **Config round-trip** (`src/config.rs::tests`): write a scaffold.toml
   with `[run] reset_localnet = true`, parse, serialize, re-parse, assert
   the field survives. Mirrors `run_config_round_trips_through_parse_serialize`.

2. **Mock project integration** (`tests/cli.rs`): set up a wallet project,
   write `[run].reset_localnet = true` to its scaffold.toml, run
   `logos-scaffold run`. Assert the build step header is reached
   (existing pattern — the run will fail at build because the mock
   project has no Cargo workspace, but config parsing must succeed and
   the pipeline must enter step 1).

3. **Reset + restart both set** (`tests/cli.rs`): same mock setup with
   *both* `restart_localnet = true` and `reset_localnet = true`. Run
   with `--restart-localnet` flag too. Assert the run still reaches
   step 1, and that no warning about the combination is emitted —
   confirming the orthogonal-input model.

4. **CLI flag conflict** (`tests/cli.rs`): `lgs run --reset-localnet
   --no-reset-localnet` exits non-zero with clap's "cannot be used with"
   message. Mirrors the existing `run_rejects_both_restart_flags` test.

No unit tests for `cmd_localnet_reset` itself — that's already covered
by the existing `reset_*` tests in `src/commands/localnet.rs`.

## Boundaries

**Always:**
- Run `cargo fmt --check` and `cargo test --all-targets` before commit.
- Match the existing `restart_localnet` patterns exactly (CLI flag pair,
  config precedence, `effective_*` naming).
- Update README.md alongside the code change.

**Ask first:**
- Before adding any new dependency (the hand-rolled config parser is
  intentional — don't bring in `toml` for this).
- Before changing the step-counter shape (e.g. moving to 7 steps).
- Before changing `cmd_localnet_reset`'s signature; it's reused by the
  `localnet reset` subcommand.

**Never:**
- Subprocess-exec `localnet reset` from `run` — call the function.
- Add an interactive confirmation prompt without explicit approval.
- Default `reset_localnet` to `true`. Destructive defaults are surprising.
- Introduce a `localnet_mode` enum that replaces `restart_localnet`.
  Two flags, documented precedence, done.

## Success Criteria

### Reset feature

- [ ] `lgs run --reset-localnet` on a real lez-framework project: wipes
      `<lez>/rocksdb`, wipes `<project>/.scaffold/wallet`, deletes
      `.scaffold/state/localnet.state` and the wallet state file, starts
      the sequencer, verifies block production, tops up wallet, deploys,
      runs hooks. Whole pipeline succeeds end-to-end.
- [ ] `lgs run --no-reset-localnet` overrides `[run].reset_localnet =
      true` in config — wallet survives.
- [ ] `lgs run --reset-localnet --no-reset-localnet` fails with clap's
      conflict error.
- [ ] `lgs run --reset-localnet --restart-localnet` succeeds; reset wins
      silently (no warning, no double-stop).
- [ ] `lgs run --reset-localnet` on a cold start (no sequencer running,
      no state file) succeeds.
- [ ] Round-trip parse/serialize preserves `reset_localnet` value.
- [ ] All existing tests still pass.
- [ ] `cargo fmt --check` is clean.
- [ ] README documents the flag and the supersession rule.

## Open Questions

None. Design closed in the one-pager (`docs/ideas/run-reset.md`); the
cold-start question was resolved by reading `cmd_localnet_stop` and
`reset_cleanup` source.
