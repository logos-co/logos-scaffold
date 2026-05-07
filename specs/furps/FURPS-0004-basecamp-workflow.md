# FURPS-0004: Basecamp Workflow

## Status
Accepted

## Functional Requirements

1. `basecamp setup` prepares pinned basecamp/lgpm tooling and profile roots.
2. `basecamp modules` captures module sources into config as source-of-truth set.
3. `basecamp install` replays captured modules into profiles.
4. `basecamp launch <profile>` runs using profile-scoped state/env.
5. `basecamp build-portable` builds/stages portable artifacts for project modules.
6. `basecamp doctor` reports basecamp-specific health/drift information.

## Determinism Requirements

1. Module capture/replay is idempotent for unchanged inputs.
2. Dependency/source resolution follows fixed precedence and fails fast when unresolved.
3. Portable staging uses stable naming/order and does not mutate profile runtime state.

## Acceptance Checks

- Missing setup prerequisite yields actionable guidance.
- Re-running modules capture with unchanged sources does not introduce unrelated churn.
- `build-portable` output is staged under scaffold-managed portable directory.
