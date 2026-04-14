# SPEC: `logos-scaffold localnet reset`

## Context

`logos-scaffold localnet reset [--keep-wallet]` — atomically resets the localnet environment to a clean state.

## Problem

Developers resetting localnet need to know undocumented internal file locations and execute deletions in the correct order. The failure mode when steps are skipped or reordered is silent: `spel` hangs forever on "Waiting for confirmation...", no errors anywhere, because sequencer on-chain nonces and wallet nonces are out of sync.

## Solution

Single command with atomic 8-step reset:
1. Stop sequencer (releases RocksDB lock)
2. Delete sequencer RocksDB at `.scaffold/cache/repos/lssa/rocksdb`
3. Delete wallet dir (unless `--keep-wallet`)
4. Delete `wallet.state` pointer (unless `--keep-wallet`)
5. Delete `localnet.state`
6. Run `setup` to recreate wallet and seed default address
7. Start sequencer
8. Poll `get_last_block` RPC until block height > 0 (30s timeout), confirming clean state

## Usage

```bash
logos-scaffold localnet reset
logos-scaffold localnet reset --keep-wallet
```

## CLI

Add to `localnet` subcommand:
- `reset` subcommand
- `--keep-wallet` flag (boolean, defaults to false)

## Files to Change

- `src/cli.rs` — add `reset` subcommand
- `src/commands/localnet.rs` — add `LocalnetAction::Reset { keep_wallet: bool }` variant and implementation
- `src/error.rs` — add `ResetError` if needed

## Constraints

- Atomic: all steps succeed or rollback (already-run steps have no rollback, but failures are reported)
- Non-fatal missing files: if a path doesn't exist, skip deletion and continue
- `--keep-wallet`: skips wallet dir and wallet.state deletion
- Timeout on block polling: 30s max, then error with helpful message
