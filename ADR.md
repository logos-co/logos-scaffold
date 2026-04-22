# Scaffold — Architecture Decision Records

## Project Structure (Single-Repo Template)

Developers need one bootstrap target that is immediately runnable and easy to modify.
Use a single generated project containing contract, CLI client, configuration, and deployment scripts.
Single-template onboarding is very simple.

## CLI

The workflow should be discoverable for new developers.
Expose one CLI surface with subcommands for init, build, deploy, and interact.
One CLI improves onboarding but makes it hard to maintain backward-compatibility.

## Local Runtime

Local development should work without requiring manually managed external node setup.
Provide embedded localnet lifecycle commands as part of scaffold workflow.
The scaffolded toolchain can start, stop, and reset a localnet environment
that supports deploy and wallet-based interaction for the generated example contract.

## Build Pipeline

Contract compilation should align with Rust ecosystem standards
and avoid unnecessary abstraction.
Use native Cargo-based build flow as the primary compilation path.

## Network Configuration

Developers need explicit, editable environment targeting for local and DevNet workflows.
Use environment-file based network configuration as the default model.
Generated projects include env files for local and DevNet,
wallet interaction settings used by deploy and interact commands.
Env files are familiar and automation-friendly,
but require strict handling to avoid credential leakage.

## Reset is Full Teardown with Re-Seed

Reverting a project's basecamp state cleanly needs to be a single command rather than
a sequence of shell tricks. `basecamp reset` kills any live basecamp, removes the
entire `.scaffold/basecamp/profiles/` tree, clears recorded sources from
`basecamp.state`, and re-seeds empty `alice`/`bob` profiles in the same run. Pinned
basecamp + lgpm binaries are preserved so the expensive `setup` step is not re-run.
Re-seeding in-line avoids a two-command reset+setup dance; the tradeoff is that
`reset` always leaves the project in the specific "seeded, no modules" state, not a
pure teardown.

## Portable Artefact Build is Separate from Install

The `.#lgx-portable` output exists to load into a released basecamp AppImage — a
different delivery path with different XDG paths and a different install mechanism
than the scaffold-managed `alice`/`bob` profiles. `basecamp build-portable` targets
that output and stops once the artefacts exist; it never invokes `lgpm`, never
mutates profile state, and never touches `basecamp.state`. This keeps the two
delivery flows strictly separate, trading a slight command-surface duplication for
clean boundaries.

## AppImage Path is Outside Scaffold's Scope

`build-portable` could have tried to auto-locate a basecamp AppImage and copy
artefacts into its module directory. It does not. The scaffold does not know the
AppImage's install location, and probing filesystem heuristics would be unreliable
across Linux distributions and macOS bundles. Instead, scaffold produces the `.lgx`
artefacts and prints their absolute store paths; the developer copies them into
whatever AppImage they are testing against. The tradeoff is an extra manual step;
the upside is no fragile heuristics and no surprise writes into user-managed
locations.

## Flake Attribute Selection is a Resolver Parameter

`install` and `build-portable` share a single source-resolution routine that is
parameterized on the flake attribute name (`lgx` vs `lgx-portable`). Hardcoding two
parallel resolvers would duplicate the precedence rules (explicit → root flake →
sub-flakes → targeted failure) and bug-surface the sibling path-input override
logic. The tradeoff is one extra argument on an internal helper; the upside is
that any resolver fix automatically applies to both commands, and the "only
`.#lgx-portable` found" and "only `.#lgx` found" error paths are symmetric by
construction.
