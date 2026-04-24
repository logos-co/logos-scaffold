# Scaffold — FURPS+

## FURPS+ (v0.1)

[v0.1 milestone](https://github.com/logos-co/ecosystem/milestone/9)

### Functionality

1. One public DevNet vertical slice: generate wallet, fund wallet, deploy contract, execute one transaction type, verify result.
2. Integrate wallet generation as part of the scaffold workflow for bootstrap and interaction flows.
3. Support native token topup for wallet operations on local and DevNet environments.

### Usability

1. Single command bootstrap with no manual project wiring required.
2. Generated layout clearly separates contract code, client code, config, and deploy scripts.
3. Deterministic wallet generation and .env handling for repeatability.
4. Clear happy-path docs, reproducible setup, discoverable commands.
5. CLI prints underlying commands for each step so users can drop down to lower-level tooling.

### Reliability

1. The vertical slice must succeed 3 times in a row on a clean machine with deterministic wallets.
2. Local network can be started and torn down in isolation without modifying host-global blockchain state.

### Performance

1. Each workshop step must complete within a demo-tolerable threshold (a few minutes).

### Supportability

1. Scaffold version and toolchain versions are explicit in generated output so projects remain buildable over time.
2. Network configuration for local and DevNet deployment is .env based config.
3. The scaffolded project includes command references for build, deploy, and interaction steps.

### + (Privacy, Anonymity, Censorship-Resistance)

- Local workflow does not require uploading source code, artifacts, or private keys to third-party services.
- CLI interaction flow works with locally controlled wallet keys and does not require custodial key management.
- Local development and testing can run fully offline from public networks.
- DevNet interaction uses explicit wallet and RPC configuration so developers can avoid accidental cross-network key reuse.

### Dependencies

#### Internal Dependencies

- Logos Core DevEx for overall developer journey alignment and terminology.
- Logos Blockchain and Logos Execution Environment for functionality.
- Wallet Module for interactions with Logos Execution Environment.

#### Runtime Dependencies

- Local network runtime availability for local deploy and interaction workflows.
- DevNet RPC endpoint availability and stable chain configuration.
- Deterministic local/DevNet account and chain configuration via environment files.

#### Wallet Dependencies

- Wallet available for signing transactions initiated by CLI interaction commands.
- Network-aware wallet configuration to prevent cross-network key misuse.
