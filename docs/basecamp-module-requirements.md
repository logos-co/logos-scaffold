# Basecamp Module Requirements

This is the contract between a module project and `logos-scaffold basecamp {setup,install,launch,reset,build-portable}`. If your project satisfies the rules below, the commands will resolve, build, and install your `.lgx` artefacts into the pre-seeded `alice` and `bob` profiles automatically. `build-portable` additionally targets the `lgx-portable` flake output for hand-loading into a basecamp AppImage.

## Hard requirements

1. **`scaffold.toml` at the project root.** Basecamp commands refuse to run outside a scaffold project. Run `logos-scaffold init` once if you don't have one.

2. **`basecamp setup` must have been run once** in the project. It pins the basecamp repo, builds the `basecamp` + `lgpm` binaries, and seeds the `alice` / `bob` profile directories under `.scaffold/basecamp/profiles/`. `install` and `launch` will emit a targeted hint if you skip this.

3. **At least one `flake.nix`** that exposes a `.lgx` package:
   - Either at the project root, or
   - In one or more immediate sub-directories (one per sub-flake).

4. **Each such flake must expose `packages.<system>.lgx`** — this is the convention established by `logos-module-builder` (tag `tutorial-v1`).
   - If a flake only exposes `packages.<system>.lgx-portable`, the resolver fails explicitly with a hint — it will not silently fall back. Expose `lgx` or pass `--flake <ref>#lgx-portable` on the command line to opt in.
   - If no flake exposes any `.lgx` attribute, the resolver fails with a generic hint pointing at `--path` / `--flake`.

## Conventions that matter for local development

Multi-flake projects (e.g. a `tictactoe` core plus `tictactoe-ui-cpp` and `tictactoe-ui-qml` sibling flakes) rely on one convention:

**Sibling sub-flakes are auto-overridden by directory name.** When scaffold builds a sub-flake, it passes `--override-input <dir-name> path:<sibling-abs-path>` for every other path-flake that shares the same parent directory — but only if the target flake actually declares `<dir-name>` as one of its inputs.

Concretely:

- Directory layout `my-module/{core,ui-cpp,ui-qml}` with `flake.nix` in each.
- `ui-cpp/flake.nix` must declare `inputs.core.url = "path:../core";` (the input name must match the sibling directory name).
- When scaffold builds `ui-cpp#lgx`, it automatically passes `--override-input core path:/abs/path/to/my-module/core`, so you get a local-source build rather than the pinned `github:` ref.
- If `ui-cpp` does not declare `core` as an input, the override is silently dropped (no nix warning). If `ui-cpp` declares `core-lib` instead, rename the input to `core` to match the directory, or rename the directory to `core-lib`.

Non-path flake refs (e.g. `github:…`) and `.lgx` path sources are never auto-overridden.

### Transitive inputs must `follows` the top-level `logos-module-builder`

Multi-sub-flake projects that pull in modules which themselves depend on `logos-module-builder` (e.g. `delivery_module` → `logos-module-builder`) **must** unify that transitive reference onto the project's top-level pin using a `follows` entry.

Without it, your sub-flake's `flake.lock` ends up with two `logos-module-builder` entries: the one you pinned and a second one pulled in transitively (typically off the upstream's `main` branch, which may be incompatible with the `basecamp v0.1.1` wire format or with the tutorial-sanctioned contract). When scaffold then runs `nix build path:<sibling> --override-input <dir-name> path:<this-sub-flake>`, nix resolves transitive inputs through **this sub-flake's lock**, and the stale second entry silently wins — builds that work with a direct `nix build .#lgx` fail with opaque errors when invoked through scaffold.

Concrete fix: in each sub-flake that declares both `logos-module-builder` and a module with its own `logos-module-builder` input, add the `follows`:

```nix
# tictactoe/flake.nix (example)
{
  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/tutorial-v1";
    delivery_module.url = "github:logos-co/logos-delivery-module/<pinned-rev>";

    # Force delivery_module's transitive `logos-module-builder` to follow our
    # tutorial-v1 pin. Without this, delivery_module drags in its own
    # master-branch module-builder (newer, incompatible with basecamp v0.1.1's
    # bundled delivery_module wire format) as a second entry in flake.lock.
    # That extra entry silently wins when a UI flake does
    # `--override-input tictactoe path:...` and breaks the tutorial-sanctioned
    # local-dev workflow.
    delivery_module.inputs.logos-module-builder.follows = "logos-module-builder";
  };
  # ...
}
```

Symptoms when this is missing:

- `lgs basecamp install` fails inside `nix build` with errors from a newer `logos-module-builder` (e.g. `no 'main' field in metadata.json`).
- `cd <sub-flake> && nix build .#lgx` works directly, because the direct build uses the sub-flake's own lock and never dereferences the extra entry.

Apply the same `follows` wiring to every transitive input that *also* pulls in `logos-module-builder`. After adding it, re-run `nix flake update` in that sub-flake and verify `flake.lock` now contains exactly one `logos-module-builder` node (or `logos-module-builder_N` aliases all resolving to the same rev).

This is a limitation of the current tutorial-era `logos-module-builder` scaffolding and is expected to be handled automatically upstream in a later release.

## Explicit escape hatch

If the resolver doesn't do what you want, pass explicit sources to `basecamp install`:

```bash
# Pre-built .lgx files
logos-scaffold basecamp install --path ./dist/my-module.lgx

# Arbitrary flake refs (including portable variant, remote refs, non-standard attrs)
logos-scaffold basecamp install --flake .#lgx-portable
logos-scaffold basecamp install --flake github:me/my-module#lgx
```

Explicit sources win over auto-discovery entirely — no root or sub-flake probing happens when `--path` or `--flake` is present.

## AppImage testing via `build-portable`

`basecamp install` / `launch` load modules into the scaffold-managed alice/bob profiles. To instead test against a released basecamp **AppImage**, use `build-portable`:

```bash
logos-scaffold basecamp build-portable
# → builds .#lgx-portable for each auto-discovered source
# → prints the absolute store paths of the built .lgx artefacts
# → leaves `./result-lgx-portable` symlinks next to each flake
```

`build-portable` does not touch profiles, `basecamp.state`, or the AppImage itself — it only produces artefacts. Copy them into your AppImage's module directory manually; scaffold is intentionally unaware of that path.

If a flake exposes only `lgx` (not `lgx-portable`), `build-portable` fails with a targeted hint — mirror of the `install` portable-only failure, in reverse.

## Quick checklist

- [ ] `scaffold.toml` exists at the project root.
- [ ] `logos-scaffold basecamp setup` has been run.
- [ ] Each sub-flake exposes `packages.<system>.lgx`.
- [ ] Sibling input names match sibling directory names, if applicable.
- [ ] Transitive `logos-module-builder` references are unified with a `follows` onto the top-level pin (see "Transitive inputs must `follows` …" above).
- [ ] No project relies on `lgx-portable` as the only output without passing `--flake` explicitly.
