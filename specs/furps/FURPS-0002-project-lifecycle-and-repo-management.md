# FURPS-0002: Project Lifecycle and Repo Management

## Status
Accepted

## Functional Requirements

1. `new/create` scaffold a project.
2. `init` bootstraps scaffold config in an existing project and is the migration entry point for outdated schema.
3. `setup` resolves configured pinned repositories and builds project-local binaries required by scaffold commands.
4. Wrapper commands (`wallet`, `spel`) execute project-resolved binaries, not global binaries.

## Determinism Requirements

1. Repository resolution must be pin-driven from project config/defaults.
2. Commands requiring project context fail with actionable message when run outside project or on outdated schema.

## Acceptance Checks

- Re-running `init` on outdated config migrates schema path rather than requiring manual rewrite.
- `setup` output and resulting binaries are project-scoped under scaffold-managed paths.
