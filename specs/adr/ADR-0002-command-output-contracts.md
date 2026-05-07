# ADR-0002: CLI Output Is a Public Contract

## Status
Accepted

## Decision

For user-facing commands, treat output shape as API:

1. Human mode: stable key lines and remediation hints.
2. JSON mode: deterministic object structure suitable for automation.
3. Introspection/help commands: no filesystem mutation.

## Rationale

Downstream CI/scripts depend on output shape, not only exit codes.

## Consequence

Output-shape changes require spec update and test update.
