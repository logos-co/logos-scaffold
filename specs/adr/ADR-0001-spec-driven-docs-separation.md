# ADR-0001: Separate Strategic Docs from Current-State Specs

## Status
Accepted

## Decision

Keep two layers:

1. Root-level `FURPS.md` and `ADR.md` for strategy/roadmap context.
2. `specs/` for concrete current-state behavior contracts.

## Rationale

Root docs include forward-looking content; reconstruction and regression review need implementation-grounded contracts.

## Consequence

When command behavior changes, update `specs/` in the same change.
