# CI process definition

States, owners, and commands for the snapshot-once freeze/unfreeze CI model (ADR 0001, as amended 2026-10-17). The invariant: runners never make choices. Brew runs only inside `freeze.yml` mac rows against the pinned `homebrew-core` commit; every consumer restores its engines from the frozen release. **Proven live 2026-10-17:** freeze run `39009234965` froze all four triples green; its finalize hit GitHub's asset-indexing race on the first live pass (counted 1/8 while uploads landed, draft left unpublished by design), the operator sha-verified all four bundles and published `sidecars-20261017` (8 assets) — the verify-race retry loop and idempotent draft reuse are now part of finalize. The gate (2 rows) and release (4 native rows) pipelines unfreeze and consume that release.

## 1. Artifact flow

```
operator                freeze.yml                         consumers
────────                ──────────                         ──────────
edit manifest ─┐                                           PR gate (ci.yml, 2 rows)
commit ────────┤   gate: tag-collision preflight           every row, per triple:
click freeze   │         ↓                                 unfreeze --triple <t>
               │   freeze matrix (4 rows, 40 min)    →    → (mac) sync-macos-frameworks
               │     mac rows: pin checkout → brew          (derives the framework list
               │       install off pinned tap → all.mjs     from the staged tree)
               │       → sync-macos-frameworks              → clean-room audit (mac rows:
               │     win rows: all.mjs (URL+sha256,            BINARIES_ALLOW_INACTIVE_BREW=1)
               │       msys2 gs closure via extraPackages)
               │       → freeze.sh --triple                 → verbatim gate
               │         ↓                                  release.yml adds:
               │   finalize (15 min): preflight 8           → tauri-action into the draft
               │     create-or-reuse today's draft          (numeric releaseId)
               │     upload 8 assets → verify 4×10s         → per-row app-asset check
               │     retry through asset-index lag          → package gate (open the built
               │                                              package, run every engine)
               │       ↓
               └──→  sidecars-<date> release (published)
                       ↓
                  draft app release → operator clicks publish
```

Every artifact is sha256-verified twice: at freeze publish (bundle ↔ sibling digest) and at unfreeze (digest match before extraction). Unfreeze leaves a provenance marker, `src-tauri/.sidecars-restored.json` (`triple` + `bundleSha256` + `restoredAt`): `verify.mjs` treats the marker as governing — it skips the `stagedSha256`-vs-lock comparison and pins tree bytes to the bundle sha verified at unfreeze. `fetch`/`prep` delete a stale same-triple marker before staging so a re-stage is never masked by an old marker.

## 2. Engine bundle shapes (proven 2026-10-17)

**mac (`aarch64-apple-darwin`, `x86_64-apple-darwin`)** — roots `binaries/` + `frameworks/` + `gs-resources/`: the 5 flat engine binaries, the vips dylib tree, and the ghostscript `Resource`/`lib`/`iccprofiles`/`fonts` tree, each validated post-unfreeze (`gs_init.ps` marker, non-empty `frameworks/`).

**windows (`x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`)** — binaries-only roots, two payloads:

- `binaries/engines/<dest>/` payload dirs for every **non-static** manifest entry (vips, qpdf, gs): engine exe + recursive DLL tree (DLL presence is enforced, not a flaky sibling check);
- flat exes `binaries/<dest>-<triple>.exe` for **every** manifest entry — the `verify.mjs` content-arch and the audit gate hold after unfreeze, and tauri consumes the flat `externalBin` pair.

Static entries (`ffmpeg`/`ffprobe`, `static: true`) ship as the flat exe only: no engines dir, no DLL scan. Validation lists derive from the manifest at consume time (`scripts/ci/windows-manifest-lists.sh`) — never hardcoded.

**Windows gs runs on the ROM model** (msys2 ucrt64 ghostscript ships no Resource tree; gs serves pdf/cs/img from its compiled-in tree):

- the windows manifest gs row declares **no `resources` key** (mac gs still does);
- its DLL closure is pinned as `extraPackages`: 25 URL+sha256 msys2 ucrt64 rows on x64, 23 on arm64 — 48 pinned packages in total, staged beside `binaries/engines/gs/`; gated by `pe-imports.mjs` + `closure-check.mjs`;
- `src-tauri/tauri.windows.conf.json` drops the base conf's gs-resources mapping with RFC 7396 null-merge (`"gs-resources/": null`) — windows bundles never stage a Resource tree;
- the audit's gs smoke runs in ROM mode: no `-I` flags, only proof that the binary renders complete PDFs from the compiled-in tree.

**Installed layout.** The macOS package puts the five engines flat in `Contents/MacOS/` (Tauri `externalBin`). The Windows packages put vips/gs/qpdf under `<install root>/resources/engines/<name>/<name>.exe` with their DLLs — matching `GLOSSARY.md` — and ffmpeg/ffprobe flat beside the app as `ffmpeg.exe`/`ffprobe.exe` (the bundler strips the target-triple suffix). The resolver probes exactly those paths (`src-tauri/src/sidecar.rs`), and the release package gate asserts them in the real package.

## 3. Operating runbook

**Version bump = one flow** (manifest/pins edit → gate → merge → freeze → tag; there is no separate "local snapshot" path — the local machine is development only).

1. Bump engine pins: for URL engines run `yarn binaries:refresh` (rewrites `scripts/binaries.manifest.json` + `scripts/binaries.lock.json`; `--dry-run` only reports; vendors rotate to evergreen `latest` tags, the assetPattern stays authoritative). For brew engines hand-edit `expectVersion` after installing locally, or follow `binaries:refresh` drift output.
2. Update the per-arch pins in `scripts/ci/homebrew-core-pins.json` **only if a manifest `expectVersion` bumped** (ADR 0001 criterion; each mac triple resolves its own sha, and the two pins move independently):
   - `aarch64-apple-darwin`: pick the newest homebrew-core commit carrying the new formula versions with `arm64_sequoia` bottles for the full closure.
   - `x86_64-apple-darwin`: leave frozen at the newest fully-sonoma-bottled closure pin (`c36952de43157c2c4a4ea05f385031aa29f30354`, 2026-08-28 — vips 8.18.6 / gs 10.07.1; per-formula bottle deaths began 2026-08-28 and completed 09-10/11). **Never bump the Intel engines unless a human re-verifies that the new versions and their whole closure carry `sonoma` x86_64 bottles** — homebrew-core stopped publishing x86_64 bottles on 2026-09-10/11, so nothing bottled after that date can ever be staged from brew on Intel. A new Intel engine source replaces this pin wholesale; it does not move forward.
   Values are hex40 sha strings, plain JSON, no comments and no extra keys.
3. `yarn build && yarn test && cd src-tauri && cargo test` locally, commit, merge.
4. Click **Freeze sidecar engines** (workflow_dispatch). Expect 4 green rows + finalize publishing `sidecars-<date>` with 8 assets. Finalize verifies with `gh release view --json assets --jq '.assets | length'`, retrying 4×10 s through GitHub's asset-indexing lag; it reuses a leftover same-day draft idempotently and never publishes anything short of the exact-8 name set.
5. Merge the release-please PR (or tag `v<version>`), then dispatch **Release** with that tag if the tag push did not start it. release-please creates a draft release; `release.yml` reuses that draft. Expect 4 green rows building into the draft, the sidecars preflight requiring all 8 bundle assets, the SBOM + third-party notices from the aarch64 row, and each row uploading its stable `Yard-*` alias + `.sha256`.
6. Operator verifies the draft and clicks publish. Never publish the `sidecars-*` release by hand.

The Release gate reuses an existing **draft** release for the tag; it fails if the release is already published. Convert a published release with `gh release edit <tag> --draft` before re-running.

**Package gate (release rows).** After `tauri-action` populates the draft and the per-row app-asset check passes, every release row runs `node scripts/ci/package-gate.mjs --triple <triple>`. It opens that row's own package, asserts every engine exists where the app resolves it, checks the manifest version, runs one real operation per engine, verifies the gs resource tree, and runs the app's own resolver against the package. A row that fails it leaves the draft unpublished — investigate the failing engine, never publish around it.

**Version bump + changelog automation (release-please, stable baseline `1.0.0`)**

`release-please.yml` runs on every push to main: it reads squash-merged Conventional Commit PRs, maintains a **draft** Release PR that bumps the version files and appends to `CHANGELOG.md`, and merges as `v<next>` (tags stay bare-`v`-prefixed via `include-component-in-tag: false`). Merging the Release PR creates the tag and a **draft GitHub release**; `release.yml` reuses that draft. Rules of the road:

- **`package.json` is the tag-version source of truth.** The release gate requires `package.json` to equal the full tag version (`v1.0.0` → `1.0.0`); the About dialog reads that same field.
- **The other version files are extra-files.** release-please updates `src-tauri/tauri.conf.json` (`$.version`) and `src-tauri/Cargo.toml` (`$.package.version`) in the same Release PR. Keep them in sync by hand only when editing the baseline outside release-please.
- **Token chain.** The workflow prefers a GitHub App (`RELEASE_PLEASE_APP_ID` + `RELEASE_PLEASE_APP_PRIVATE_KEY`), then the `RELEASE_PLEASE_TOKEN` PAT, then `GITHUB_TOKEN`. `GITHUB_TOKEN` needs the repo setting **Allow GitHub Actions to create and approve pull requests**, and events it creates do not trigger `ci.yml` or `release.yml`; the Release PR then has no gate rows and the tag does not start the Release workflow. Use the App or PAT for full automation. With `GITHUB_TOKEN`, reopen the Release PR to get its gate rows and dispatch `release.yml` with the tag after merging.
- Merging a PR into main does **not** release; only merging the draft Release PR creates the tag and draft release, then the Release workflow builds into it (~135 billed min). Close the Release PR if the build is unwanted — release-please aggregates everything into the next proposal.
- The baseline is `1.0.0` (`.release-please-manifest.json` = the live tag); the `1.0.0` changelog entry is hand-written and curated, and predates the baseline. Later releases use commit-derived entries; noise types (`docs`, `chore`, `ci`, `refactor`, `test`, `build`, `style`) are hidden by `release-please-config.json`.
- Squash-merge only (repo settings enforce it; merge commits are disabled) — one PR = one changelog entry; a PR title is the changelog line, so write them for humans.

**Stable download aliases.** Each release row uploads its installer again under a fixed name — `Yard-macOS-arm64.dmg`, `Yard-macOS-x64.dmg`, `Yard-Windows-x64-setup.exe`, `Yard-Windows-arm64-setup.exe` — plus a `.sha256` file. The README download buttons use `releases/latest/download/<alias>`, so they keep working across versions. The upload step finds the installer matching the release version, so a stale bundle cannot be published under the alias.

**Windows-target compile gate (developer-time, runs on the mac)**

The CI windows row compiles the `src-tauri` tests as `cargo test` on `x86_64-pc-windows-msvc`; the mac can catch every one of those compile errors before push. `cargo check --all-targets --target aarch64-pc-windows-msvc` compiles + typechecks without linking (no MSVC needed). Making the tauri-build build script pass is all that is required:

1. One-time: `rustup target add aarch64-pc-windows-msvc` + `brew install llvm` (llvm-rc cross-compiles the Windows app resource that tauri-build embeds, brew clang preprocesses it for the msvc triple).
2. Stage gitignored placeholders then check, then remove them — tauri-build validates `externalBin` + the windows-conf `binaries/engines` mapping at build-script time, and the mac-native `sidecars_are_self_contained` test must not see leftover placeholder files (from the repo root):
   ```bash
   for b in vips qpdf gs ffmpeg ffprobe; do : > src-tauri/binaries/${b}-aarch64-pc-windows-msvc.exe; done && mkdir -p src-tauri/binaries/engines
   cd src-tauri && env RC_aarch64-pc-windows-msvc=/opt/homebrew/opt/llvm/bin/llvm-rc CC_aarch64-pc-windows-msvc=/opt/homebrew/opt/llvm/bin/clang cargo check --all-targets --target aarch64-pc-windows-msvc; cd ..
   rm -f src-tauri/binaries/*-aarch64-pc-windows-msvc.exe
   ```

First local reproduction (2026-10-17): gate run 39022660502's full windows cargo failure = 36 E0425 in `sidecar.rs` + `processors/document.rs`, fixed by extending the repo `#[cfg(unix)]` precedent to the consuming fns/tests. Gate before every push touching `src-tauri/src/`. The windows ci row additionally carries loader diagnostics steps (cargo-test link probe, PE-imports probe, test-enumeration loader probe) for the fixed 0xc0000139 loader class — kept until the removal decision tracked in `docs/bugs.md`.

**Recovery**

- Same-day `sidecars-<date>` collision: delete the leftover draft; `gh release delete <tag> --cleanup-tag --yes`; re-run the failed freeze rows.
- Finalize yellow-draft: `gh release delete <tag> --cleanup-tag --yes`, re-run finalize (finalize reuses a leftover draft idempotently only while it is still a draft).
- Release draft leftovers: `gh release delete <tag> --cleanup-tag --yes`, re-run the release.

## 4. Failure classification

| Symptom | Meaning | Remedy |
| --- | --- | --- |
| freeze mac row: drift gate FAIL | pin's formulas no longer match manifest `expectVersion` | operator: refresh the pin (new commit) or fix the manifest — never hide the drift |
| freeze mac row: brew install slow/hanging | pin lacks bottles for this closure | bounded by 40-min timeout. arm rows: re-pin (newest commit with `arm64_sequoia` bottles). Intel rows: the pin's closure is partially source-building — restore the newest fully-sonoma-bottled closure pin (currently `c36952de4315`); never push Intel engines forward without human bottle re-verification (ADR 0001) |
| freeze mac row: git ops on the tap fail | dirty/partial tap checkout from a prior run | pin step resets+cleans the tap, then fetches/checkout the pin; clone errors name the recovery |
| finalize: found N/8 after the 4-retry loop | a matrix row failed quietly or artifacts dropped | re-run failed rows / finalize; no partial-sidecars publishes exist (finalize reuses the draft; never publishes incomplete) |
| consumer: unfreeze FAIL / no sidecars-* | freeze never ran, or wrong newest release | run freeze.yml first (release preflight catches this earlier) |
| consumer: sha256 mismatch | torn or tampered publish | delete + re-run freeze finalize |
| consumer: audit FAIL | frozen engines wrong arch/version/stripping vs manifest | freeze is stale vs manifest: re-run freeze after pin/manifest reconcile |
| freeze/unfreeze dll-scan under git-bash: "too few X's in template" | git-bash mktemp demands trailing X's | keep the `mktemp -t <name>-XXXXXX` template shape (trailing X's) |
| git-bash tar: "C:" host/remote errors | GNU tar parses `C:/path` drive-colons as a remote host | extract with the native System32 `tar.exe` (`lib.mjs` `tarToolForHost`), never git-bash GNU tar paths |
| bash 3.2: ERR trap fires inside guarded pipelines | macOS default bash 3.2 ERR semantics differ | guard the trap with a re-entrancy flag (`err_trap_active` in `scripts/audit-sidecars.sh`) so guarded failures do not double-fire |
| bash 3.2 + `set -u`: "unbound variable" on `"${arr[@]}"` | empty array expands as unset on bash 3.2 | expansion guards `${arr[@]+"${arr[@]}"}` or ensure non-empty construction before expansion |
| brew 7.x: `brew list --versions` shapes differ (7.0.9) | brew 7 prints nothing/keg-different text for a missing formula | verify.mjs classifies `no such keg`/empty output; on brew-less CI hosts the audit only proceeds with `BINARIES_ALLOW_INACTIVE_BREW=1` note lines |
| windows test exe dies pre-main with 0xc0000139 | `STATUS_ENTRYPOINT_NOT_FOUND` — the app's embedded exe manifest was missing from cargo test executables | every msvc link (tests included) embeds `src-tauri/windows-app-manifest.xml` via `src-tauri/build.rs` (`/MANIFEST:EMBED` + `new_without_app_manifest()`, no double manifest); ci.yml keeps loader probes while the diagnostics removal decision is open |
| windows loader probe: lib-test exe refuses to launch (ENOENT) | libuv resolves the child from the CHILD env PATH | resolve `bash` absolutely from the parent PATH on win32 (`where bash`), POSIX side byte-identical |
| gate rows red beyond staging | code broken | normal dev-fix flow |
| tauri-action row: no app assets | build/upload skipped | check tauri-action logs, re-run release |
| package gate: `FAIL <engine> — file not found` | the built package does not carry that engine where the app looks | check the bundle resource map for that platform and the engine's `dest` name in `scripts/binaries.manifest.json` |
| package gate: `FAIL <engine> — Library not loaded` (macOS) | the helper cannot load the dylibs it ships with | check the ad-hoc signature and `src-tauri/Entitlements.plist`; `codesign -dvvv` and `codesign -d --entitlements -` on the helper and a bundled dylib |
| package gate: `FAIL <engine> — expected 'X' in version output, got: …` | the package carries a stale or wrong engine | re-run the freeze, then the release (ADR 0001) |
| package gate: `FAIL gs-resources` | the macOS package's `Contents/Resources/gs-resources` is missing, partial, or hollow | restage the tree (`yarn binaries:prep`) and re-freeze; never publish with gs unable to find its own fonts |
| package gate: `7z not found` (Windows rows) | the runner image no longer ships 7-Zip at the expected path | install 7-Zip in the row or pin `SEVEN_ZIP`; the gate names the expected path in its error |

## 5. Invariants

1. **CI never brews outside freeze.yml** — consumers only unfreeze; any `brew install`/`brew update`/`all.mjs` in ci.yml/release.yml is a regression.
2. **Every engine artifact is sha-verified** — manifest URL pins, freeze digests, unfreeze checks.
3. **Releases are always draft-first** — `sidecars-*` and app drafts publish only after verified assets; finalize counts assets live via `--json assets --jq` and reuses (never replaces) a leftover draft.
4. **Every job carries `timeout-minutes`** — freeze 40, release rows 90, gate rows 60, cheap jobs 10–15.
5. **No raw `${{ }}` in run strings** — untrusted context enters via step `env`; actions stay SHA-pinned; `shell: bash` everywhere.
6. **Restore provenance governs verification** — the unfreeze marker pins tree bytes to the verified bundle sha; staging clears stale markers. Open CI follow-ups (mv-shim cygpath, win32 lock-suite skips, SBOM DLL licenses, staging litter, diagnostics removal): `docs/bugs.md`.
