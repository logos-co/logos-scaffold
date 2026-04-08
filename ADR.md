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
