# SPEC: Compile Guest Binaries in methods/ Directory

## Overview

When running `logos-scaffold build` on a project that uses the LEZ framework (or any Risc0-based project), the build command must also compile guest binaries in the `methods/` directory. Currently `cargo build --workspace` skips the `methods/` directory because Risc0 guest crates target `riscv32im-risc0-zkvm-elf`, which is intentionally excluded from the main workspace build.

## What the Fix Does

After building the main workspace, `logos-scaffold build` will:

1. Detect if a `methods/` directory with a `Cargo.toml` exists in the project root
2. If found, run `cargo build --release --manifest-path <project>/methods/Cargo.toml` to compile the guest binaries

## Detection Logic

- Check for `<project_root>/methods/Cargo.toml` (i.e., `cwd/methods/Cargo.toml`)
- If it exists, the `methods/` package is a valid buildable target
- No heuristics for alternate locations (stick to `methods/` which is the Risc0 convention)

## Compilation

- Uses `--release` flag to match the host binary build mode
- Uses `--manifest-path` pointing to `methods/Cargo.toml` to ensure the correct crate is built
- Inherits any existing build flags parsed by the command (currently none are passed through)

## Error Handling

- If the `methods/` Cargo build fails, the entire `logos-scaffold build` command fails
- Failure message clearly indicates which build step failed

## Implementation

### File: `src/commands/build.rs`

Add a new function `build_methods_guests` that:
1. Checks if `cwd.join("methods").join("Cargo.toml")` exists
2. If yes, runs `cargo build --release --manifest-path <cwd>/methods/Cargo.toml` using `run_checked`
3. Call this function from `build_workspace_for_current_project` after the workspace build succeeds

## No Changes Required

- `src/lib.rs` — no new exports needed
- Templates — no changes
- Tests — cargo check validates syntax

## Acceptance Criteria

- [ ] `cargo check` passes on the modified `build.rs`
- [ ] The detection is non-fatal if `methods/` is absent (i.e., works for non-Risc0 projects)
- [ ] The detection is non-fatal if `methods/Cargo.toml` is absent
- [ ] Commit includes only the `build.rs` change (or `Cargo.lock` update if any)
