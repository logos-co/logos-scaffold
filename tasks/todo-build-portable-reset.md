# TODO — `basecamp build-portable` and `basecamp reset`

Tracks the tasks defined in [`plan-build-portable-reset.md`](./plan-build-portable-reset.md). Check off as completed.

## Phase A — Resolver parameterization

- [x] A1 Thread `attr: &str` through `classify_flake_dir` and `resolve_install_sources`; `install` passes `"lgx"`; add two new unit tests for portable attr selection and missing-attr failure. *(commit 8ae0505)*

### Checkpoint A
- [x] `cargo test && cargo fmt --all --check && cargo build` green.
- [x] No behavior change for `install`.

## Phase B — `reset`

- [x] B1 CLI surface: `BasecampSubcommand::Reset` + `--dry-run`; `BasecampAction::Reset`; stub handler. *(commit 3bfa499)*
- [x] B2 Implement `cmd_basecamp_reset`: enumerate live PIDs, print plan, kill tree, rm profiles, clear sources, re-seed alice/bob. *(commit 8b51574)*
- [x] B3 Unit tests: plan snapshot + kills-none variant, `remove_profiles_root_succeeds_on_missing_dir`, `remove_profiles_root_wipes_existing_tree`, `remove_profiles_root_refuses_symlink_escape`, `clear_basecamp_sources_preserves_binaries_and_pin`. *(commit 8b51574)*
- [x] B4 Integration tests in `tests/cli.rs`: before-setup-hint, dry-run non-destructive, wipes-and-clears-sources. *(commit ab8b256)*
- [~] B5 README bullet added *(commit 9b732fa)*; dogfood on tictactoe pending user.

### Checkpoint B
- [x] `cargo test && cargo fmt --all --check && cargo build` green.
- [ ] Dogfood transcript captured.

## Phase C — `build-portable`

- [ ] C1 CLI surface: `BasecampSubcommand::BuildPortable` + `--path` + `--flake`; `BasecampAction::BuildPortable`; stub handler.
- [x] C2 Implement `cmd_basecamp_build_portable`: parameterized resolver with `"lgx-portable"`; invoke `nix build <ref> --print-out-paths` (no `-o`/`--no-link`, per spec); pass `--path` sources through; print absolute paths + manual-copy reminder. *(commit 9835715)*
- [x] C3 Unit tests: argv cwd-override for path refs, remote-ref cwd preservation, no `--out-link`/`--no-link`, override insertion order, `--path` source validation. *(commit 9835715)*
- [x] C4 Integration tests: outside-project (C1), `--dry-run` rejected by clap (C1), inside-empty-project resolver hint (C4). *(commits 923240b, 3202e0a)*

  Note: spec's "[basecamp] section missing" test was dropped — `build-portable` does not require the `[basecamp]` section, since it never reads `basecamp.state`. Documented as a scope decision.
- [~] C5 README bullet + docs update landed *(commit 3202e0a)*; dogfood on tictactoe pending user.

### Checkpoint C
- [x] `cargo test && cargo fmt --all --check && cargo build` green.
- [ ] Dogfood transcript + profile-tree byte-identity confirmed.
- [ ] PR opened with spec link.
