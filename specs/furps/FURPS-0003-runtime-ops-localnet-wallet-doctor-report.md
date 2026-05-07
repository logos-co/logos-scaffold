# FURPS-0003: Runtime Operations

## Status
Accepted

## Functional Requirements

1. `localnet` supports `start|stop|status|logs|reset`.
2. `wallet` supports list/topup/default management and raw argument pass-through.
3. `doctor` supports human output and `--json` output.
4. `report` creates a sanitized diagnostic archive with manifest and warnings.

## Output/Contract Requirements

1. `localnet status --json` and `doctor --json` must emit machine-readable JSON.
2. `report` archive must exclude wallet key material.
3. `report` supports default output path and `--out` override.
4. `deploy --json` behavior must remain deterministic by code path (single `--program-path` object vs discovery list object).

## Safety/Reliability Requirements

1. Localnet state handling must distinguish managed state from external listeners.
2. Diagnostics collection must be best-effort and explicit about skipped data.

## Acceptance Checks

- Report archive includes manifest and diagnostics files, and omits `.scaffold/wallet/**`.
- `doctor --json` and `localnet status --json` are parseable with `jq`.
