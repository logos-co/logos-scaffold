# Spec Gap Analysis

## Objective

List concrete under-specified public behaviors and where they are now specified.

## Inputs Reviewed

- `src/cli.rs`
- `src/commands/*.rs`
- `tests/cli.rs`
- `README.md`
- `DOGFOODING.md`
- `docs/basecamp-module-requirements.md`

## Gaps and Closures

1. **CLI surface was spread across README/help/tests, not one contract.**
   - Closure: `FURPS-0001`.

2. **Lifecycle determinism (`init` migration role, `setup` vendoring scope) was fragmented.**
   - Closure: `FURPS-0002`.

3. **Machine-facing output contracts (`--json`, report archive expectations) were implicit.**
   - Closure: `FURPS-0003` and `ADR-0002`.

4. **Basecamp workflow had deep docs but no concise command-level contract.**
   - Closure: `FURPS-0004`.

5. **No rule for strategic docs vs implementation specs.**
   - Closure: `ADR-0001`.

## Remaining Gaps

1. Add schema files (or explicit schemas in docs) for JSON outputs (`deploy`, `doctor`, `localnet status`, `report` manifest).
2. Add a traceability table mapping each FURPS requirement to tests.
