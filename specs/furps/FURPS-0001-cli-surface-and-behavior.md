# FURPS-0001: CLI Surface and Behavior

## Status
Accepted

## Functional Requirements

1. Binaries: `logos-scaffold` and `lgs` are equivalent entry points.
2. Public command groups include: `create/new`, `init`, `setup`, `build`, `deploy`, `localnet`, `wallet`, `spel`, `basecamp`, `doctor`, `report`, `completions`.
3. `create` and `new` are aliases with shared flags.
4. `completions` supports `bash` and `zsh`.
5. Hidden self-test commands are excluded from normal help output.

## Behavioral Requirements

1. `--help` must be side-effect free.
2. Help text must expose documented options for each command.

## Acceptance Checks

- `create --help` does not create files.
- `wallet --help` includes `list`, `topup`, and `default`.
- `deploy --help` shows optional program argument semantics.
- `report --help` includes `--out` and `--tail`.
