# {{project_title}}

This project was generated with:

```bash
logos-scaffold new <name> --template basecamp-qml
```

It targets the pure `ui_qml` Basecamp plugin workflow.

## Local Development

```bash
logos-scaffold setup
logos-scaffold build
logos-scaffold install
```

Launch Basecamp against the scaffold-managed local data root:

```bash
export LOGOS_DATA_DIR="{{basecamp_data_root}}"
```

If your Basecamp binary is a non-portable dev build, it will load plugins from
`$LOGOS_DATA_DIR` with `Dev` appended automatically. Portable builds use the
data root directly.

## Optional Packaging

```bash
logos-scaffold build --artifact all
```

That runs the raw staging flow and then builds:

- `nix build .#lgx`
- `nix build .#lgx-portable`

## Files

- `Main.qml` — QML entry point
- `metadata.json` — Basecamp plugin metadata
- `icons/app.svg` — sidebar icon
- `flake.nix` — optional packaging and Nix-based outputs
