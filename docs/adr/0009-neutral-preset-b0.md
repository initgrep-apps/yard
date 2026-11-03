# Neutral palette: shadcn preset `b0` (nova)

The app ships preset `b0` — `nova` style, neutral palette, Inter, default radius, subtle/default menus — replacing the rhea/green preset `bNRvlrmDx`. The UI had already drifted from its declared preset (`components.json` said `iconLibrary: lucide` while `bNRvlrmDx` resolves `remixicon`), which made every `shadcn apply` an unpredictable hand-merge. Adopting the preset we actually run is what keeps the CLI's complete overwrite a repeatable upgrade path.

## Considered Options

- **Preset `b0` (chosen)** — neutral oklch tokens, `nova` component shapes, Inter, lucide; matches what the CLI writes, so `apply b0` is idempotent.
- **Keep `bNRvlrmDx` (rhea, green)** — keeps the green brand, but the preset resolves remixicon and bold inverted menus; every apply reverts icons and menu treatment.
- **Hand-maintained theme, no preset** — no CLI upgrade path; component updates and token changes drift apart silently.

## Consequences

- A complete overwrite (`shadcn apply`) replaces whole component files, so local edits to stock components are lost and must be restored after every apply: `table.tsx` keeps its `tableClassName` prop, `toggle-group.tsx` keeps the `<Value extends string>` generic, `spinner.tsx` keeps its hardened props (no `children`, `role`, `aria-hidden`) and GPL header, and every overwritten file keeps the project licence header.
- `scripts/verify-theme-tokens.sh` and `scripts/verify-dev.sh` pin the neutral tokens (`--primary: oklch(0.205 0 0)` light, `oklch(0.922 0 0)` dark, `--radius: 0.625rem`). Update them in the same commit as any future preset change.
- Preset codes do not encode the primitive base. Run preset commands inside the project; in a scratch directory pass `--base base` explicitly.
- `b0` resolves `iconLibrary: lucide`; a preset that resolves remixicon reverts the icon tree (ADR 0008).
- `shadcn apply` shells out to the package manager to install preset dependencies, so it needs the Node major the toolchain supports (Node 22 here; Node 25 trips jsdom's engine gate and aborts the apply).
