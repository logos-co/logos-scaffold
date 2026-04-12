# SPEC.md — `logos-scaffold localnet reset`

## 1. Overview

`logos-scaffold localnet reset` tears down the running localnet state and re-initialises a clean environment. It is the single command that replaces the manual procedure of deleting internal scaffold directories in the correct order, re-running `setup`, and restarting the sequencer.

### Why ordering matters

The sequencer's RocksDB database lives **inside** the cached LSSA checkout at `.scaffold/cache/repos/lssa/rocksdb`. Developers who try to "just delete `.scaffold/`" do not touch the sequencer database because it is not under `.scaffold/`. The sequencer then re-opens the stale database on the next start, causing transactions to fail silently — `spel` hangs forever with no errors. This command enforces the correct deletion order:

1. Stop sequencer (so DB is not locked)
2. Delete DB
3. Delete wallet
4. Delete state pointers
5. Re-run setup (which re-creates wallet and seeds address)
6. Start sequencer
7. Verify block production

Skipping step 1 (stopping first) can leave the RocksDB in a locked state or produce inconsistent deletion behaviour across platforms.

## 2. CLI Interface

### Command tree

```
logos-scaffold localnet reset [--keep-wallet]
```

### Flags and arguments

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--keep-wallet` | flag (no value) | `false` | Preserve the existing wallet and wallet state; only reset the sequencer DB and re-run setup. Skips deletion of `.scaffold/wallet/` and `.scaffold/state/wallet.state`. |

### Help text

```
logos-scaffold localnet reset
    Reset localnet to a clean state: stop the sequencer, delete the sequencer
    database, delete wallet state, re-run setup, restart the sequencer, and
    verify block production.

    Use --keep-wallet to preserve the existing wallet (useful when you want to
    reset the sequencer DB without losing your wallet keypairs).

    Run this when 'spel' hangs, transactions fail silently, or the sequencer
    is producing blocks with stale state.

Options:
    --keep-wallet    Skip wallet deletion; only reset the sequencer DB and state
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Reset completed successfully; blocks are being produced |
| 1 | Any step failed (see error message for details) |

## 3. File Deletion Map

All paths are relative to the project root (the directory containing `scaffold.toml`, discovered via `project.rs:find_project_root`).

| Path | Deleted always? | Deleted with `--keep-wallet`? | Reason |
|------|----------------|-------------------------------|--------|
| `.scaffold/cache/repos/lssa/rocksdb/` | Yes | Yes | The sequencer's RocksDB. Must be wiped to eliminate stale state that causes silent tx failures. Lives inside the LSSA checkout, not under `.scaffold/`. |
| `.scaffold/wallet/` | Yes | No | Wallet keypairs and nonce tracking. Deleted to ensure a clean wallet state. |
| `.scaffold/state/wallet.state` | Yes | No | Wallet state pointer (contains `default_address=...`). Deleted so setup re-seeds the address from the fresh wallet config. |
| `.scaffold/state/localnet.state` | Yes | Yes | Sequencer PID tracking. Deleted by `localnet stop` during step 1, and again here if it somehow persists. |

**The log file** `.scaffold/logs/sequencer.log` is **not** deleted. It is retained for post-mortem inspection when something goes wrong.

## 4. Step-by-Step Behaviour

### Preamble

1. Discover project root via `find_project_root(cwd)`. If not found, abort with:
   ```
   Not a logos-scaffold project at <cwd>. Run `logos-scaffold create <name>` first.
   ```
2. Load project config via `load_project()`.

### Step 1 — Stop sequencer

Call `cmd_localnet(LocalnetAction::Stop)`.

- If the sequencer is running: prints `$ kill <pid>` and terminates it.
- If no sequencer is running: prints `localnet not running` and continues. This is **not** an error.
- If a foreign listener is detected on the port: prints a warning and does not stop it. Continues.

After this step the sequencer process is confirmed dead. The `localnet.state` file is removed by `cmd_localnet_stop`.

### Step 2 — Delete sequencer RocksDB

```
rocksdb_path = lssa_path / "rocksdb"   // lssa_path = project.config.lssa.path
```

- If `rocksdb_path` exists: delete it recursively.
- If `rocksdb_path` does not exist: this is **not** an error. Log `sequencer DB not found at <path>; skipping deletion`.
- On any OS-level deletion error (permission, file locked): abort with the underlying error message.

### Step 3 — Delete wallet (unless `--keep-wallet`)

```
wallet_path = project.root / project.config.wallet_home_dir   // default: .scaffold/wallet
```

- If `--keep-wallet` is set: skip entirely.
- If `wallet_path` exists: delete it recursively.
- If `wallet_path` does not exist: this is **not** an error. Log `wallet not found at <path>; skipping deletion`.

### Step 4 — Delete wallet state (unless `--keep-wallet`)

```
wallet_state_path = project.root / ".scaffold/state/wallet.state"
```

- If `--keep-wallet` is set: skip entirely.
- If `wallet_state_path` exists: delete it.
- If it does not exist: this is **not** an error.

### Step 5 — Delete localnet state (if exists)

```
localnet_state_path = project.root / ".scaffold/state/localnet.state"
```

- If `localnet_state_path` exists: delete it.
- This is a safety cleanup — `cmd_localnet_stop` in step 1 already removes this file, but this step catches edge cases where stop was skipped or the file survived.

### Step 6 — Run setup

Call `cmd_setup(SetupCommand { wallet_install: WalletInstallMode::Auto })`.

- This syncs the LSSA repo, builds `sequencer_runner`, and (re)creates the wallet.
- If `--keep-wallet` was passed: the wallet directory already exists (step 3 was skipped), so `setup` will see the existing wallet config and skip re-creation.
- `setup` seeds the default wallet address to `wallet.state` if no default address is recorded.
- If `setup` fails: abort with the error. The environment is left in a partially reset state. The sequencer is not running at this point.

### Step 7 — Start sequencer

Call `cmd_localnet(LocalnetAction::Start { timeout_sec: 20 })`.

- This patches `sequencer_config.json` with the configured port, spawns the sequencer, writes `localnet.state`, and waits for the port to open.
- On port conflict: abort with a message directing the user to stop the conflicting process.
- On `MissingSequencerBinary`: abort with a message suggesting re-running `setup`.
- On `StartTimeout` or `ExitedBeforeReady`: abort with the last log lines.

### Step 8 — Verify block production

Poll `wallet_support::rpc_get_last_block(sequencer_addr)` every 500 ms for up to **30 seconds**.

- `sequencer_addr` = `127.0.0.1:<port>` (port from project config, default 3040).
- If `rpc_get_last_block` returns `Err(RpcReachabilityError::Connectivity(_))`: treat as "not ready yet" and continue polling.
- If `rpc_get_last_block` returns an error that is **not** connectivity-related: abort with the error.
- If the returned block height is `> 0`: verification complete. Print:
  ```
  localnet reset complete; sequencer producing blocks (block_height=<N>)
  ```
- If the 30-second deadline expires with no block: abort with:
  ```
  sequencer started but is not producing blocks after 30s.
  Check 'logos-scaffold localnet logs --tail 200' for errors.
  Run 'logos-scaffold localnet status' for diagnostics.
  ```

## 5. Edge Cases

### Sequencer not running at step 1

Not an error. `cmd_localnet_stop` prints `localnet not running` and returns `Ok`. The reset continues — the DB, wallet, and state files are still deleted, and the environment is rebuilt from scratch.

### Wallet already absent before step 3

Not an error. `setup` in step 6 creates a brand new wallet from the LSSA debug config.

### RocksDB already absent before step 2

Not an error. The sequencer is started fresh and creates a new DB on first run.

### Port conflict on restart (step 7)

`cmd_localnet_start` detects the conflict before spawning and aborts with:
```
cannot start localnet: port 3040 is already in use (pid=<N>)
This may be a sequencer started from another project and may not work with the current project.
Stop that process and retry `logos-scaffold localnet start`.
```
The reset operation fails but leaves the environment in a stopped state. The user must resolve the port conflict and re-run `reset`.

### `setup` fails (step 6)

Abort. The environment is in a partially reset state (DB and old wallet deleted, new wallet not created). The sequencer is not running. Re-running `reset` is safe and will retry `setup`.

### Sequencer exits before ready (step 7)

`cmd_localnet_start` catches this and aborts with the last 60 log lines. The `localnet.state` file is removed before the error is returned. The sequencer is not running. Re-running `reset` is safe.

### Sequencer does not produce blocks after start (step 8)

Abort after 30 seconds of polling. The sequencer is running and the port is open, but it is not finalising blocks. The log file should be inspected. This is the "spel hangs" symptom — at this point the user should re-run `reset` after checking the logs, or file a bug with `logos-scaffold report`.

### `--keep-wallet` with no existing wallet

If `--keep-wallet` is passed but `.scaffold/wallet/` does not exist (wallet was never created or already deleted): step 3 and 4 are skipped, `setup` creates a fresh wallet in step 6, and the reset completes normally. The `--keep-wallet` flag is a no-op in this case.

### `rpc_get_last_block` returns HTTP error (non-2xx)

Mapped to `RpcReachabilityError::Other`. This causes an abort in step 8, not a retry. The sequencer is reachable but returned an error response, which is unexpected and worth surfacing.

## 6. Verification

After step 7 (`localnet start`), step 8 polls `get_last_block` via HTTP POST to `127.0.0.1:<port>`.

**Success:** `result.last_block > 0` returned within 30 seconds.

**Manual verification:**
```bash
logos-scaffold localnet status
# ready: true  → sequencer is producing

# Or directly:
curl -s -X POST http://127.0.0.1:3040 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_last_block","params":{}}' \
  | jq .result.last_block
```

## 7. Interaction with Existing Commands

| Command | Relationship |
|---------|--------------|
| `logos-scaffold localnet start` | `reset` calls this internally in step 7. All start semantics (port check, readiness wait, `localnet.state` write) apply. |
| `logos-scaffold localnet stop` | `reset` calls this internally in step 1. Handles the "not running" case gracefully. |
| `logos-scaffold localnet status` | Used by `reset` only for pre-flight checks in the current implementation. After `reset`, `status` should report `ready: true`. |
| `logos-scaffold localnet logs` | Not called by `reset`. Available for post-failure diagnosis. `reset` failure messages reference it. |
| `logos-scaffold setup` | `reset` calls this internally in step 6. It is the canonical setup operation. `reset` does not pass `--wallet-install never` because the wallet may need to be (re)installed. |

## 8. Implementation Notes

### Data structures

Add to `src/commands/localnet.rs`:

```rust
pub(crate) enum LocalnetAction {
    // ... existing variants ...
    Reset { keep_wallet: bool },
}
```

Add to `src/cli.rs` in `LocalnetSubcommand`:

```rust
enum LocalnetSubcommand {
    // ... existing variants ...
    Reset(LocalnetResetArgs),
}

struct LocalnetResetArgs {
    #[arg(long)]
    keep_wallet: bool,
}
```

### New error variant

In `src/error.rs`, add:

```rust
#[derive(Debug, Error)]
pub(crate) enum ResetError {
    #[error("sequencer started but is not producing blocks after 30s.\nCheck 'logos-scaffold localnet logs --tail 200' for errors.\nRun 'logos-scaffold localnet status' for diagnostics.")]
    BlocksNotProduced,

    #[error("verification poll failed: {0}")]
    VerificationPollFailed(String),
}
```

### Key paths

All paths use the already-existing helpers and project config fields:

- `project.config.lssa.path` — LSSA checkout root (e.g. `.scaffold/cache/repos/lssa`)
- `project.config.wallet_home_dir` — wallet directory name (e.g. `.scaffold/wallet`)
- `wallet_state_path(project_root)` — from `wallet_support.rs`
- `project.root / ".scaffold/state/localnet.state"` — localnet state path

### `rpc_get_last_block` reuse

The `rpc_get_last_block` function in `wallet_support.rs` is already used by the topup logic and is suitable for step 8 verification without modification.

### Ordering invariant

The ordering of steps 1–5 is load-bearing. Any refactor must preserve:
1. Stop before deleting DB
2. Delete DB before setup
3. `setup` must run before `start`
4. Verification must follow `start`
