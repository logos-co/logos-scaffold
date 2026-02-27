# Security Model

## Development-Only Wallet Material

`logos-scaffold` is designed for local standalone development flows.

- Wallet home: `.scaffold/wallet`
- Default wallet state: `.scaffold/state/wallet.state`
- Localnet address: `http://127.0.0.1:3040`

Do not use scaffold-generated wallets or keys for real funds or production environments.

## Deterministic Local Password

Scaffold wallet automation uses a deterministic local password by default for onboarding UX.

Override with:

```bash
export LOGOS_SCAFFOLD_WALLET_PASSWORD='<your-local-dev-password>'
```

This override applies to scaffold commands that submit wallet password input (`wallet`, `deploy`, `doctor` checks).

## Repository Hygiene

- Keep `.scaffold/` out of source control.
- Generated projects also ignore `.env.local` by default.
- Treat wallet config and local logs as sensitive development artifacts.

## Network Behavior

Scaffold does not include telemetry.
Network activity is limited to explicit operations you trigger (for example, syncing pinned repositories or calling configured local endpoints).
