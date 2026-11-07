# Contributing to Yard

Thanks for helping. Yard is a small, opinionated project: the Rust core does all
compression work and the React frontend is UI only.

## Before you start

- **Bug or feature?** Open an issue first for anything larger than a typo or a
  one-line fix. It saves a rejected PR.
- **Security issue?** Follow [`SECURITY.md`](SECURITY.md). Never open a public
  issue for a vulnerability.
- By contributing you agree to the [Code of Conduct](CODE_OF_CONDUCT.md) and to
  license your work under GPL-3.0.

## Development setup

Requirements: Node.js 22, [yarn](https://yarnpkg.com/), Rust stable, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your
platform.

```bash
yarn install
yarn binaries:all     # one-time: download, verify, and stage the engines (~150 MB)
yarn tauri dev
```

The sidecar engines are gitignored. `yarn binaries:all` populates
`src-tauri/binaries/` from the manifest and lock files in `scripts/binaries/`.
Never commit engine binaries.

## The gate

Every PR must pass the same command CI runs:

```bash
yarn build && yarn test && (cd src-tauri && cargo test)
```

Run it locally before pushing. A PR with a red gate is not reviewed.

Windows-target changes also need the cross-compile check documented in
[`docs/cicd-process.md`](docs/cicd-process.md).

## Making a change

1. Branch from `main`.
2. Keep the change focused. Separate refactors from behavior changes.
3. Add or update tests for behavior changes; fix the root cause rather than the
   symptom.
4. Match the surrounding style. No comments unless the code cannot explain
   itself.
5. Use [Conventional Commits](https://www.conventionalcommits.org/):

   ```
   feat(compress): add per-batch output folder
   fix(history): keep the selected batch after a refresh
   chore(ci): pin the release runner
   ```

   The PR title becomes the changelog line, so write it for humans.

6. Open a PR using the template. Fill in what changed, why, and how you tested
   it.

## Pull request review

- `main` is protected; merges require the two native gate rows.
- Squash-merge only. One PR becomes one release-please changelog entry.
- Address review comments with new commits; do not force-push during review.
- Do not include personal data in screenshots, fixtures, logs, or commit
  messages. Demo data must use generic file names and paths.

## Releases

Releases are automated with `release-please`:

- Every merged Conventional Commit PR feeds the next Release PR.
- Merging the Release PR bumps `package.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml`, and `CHANGELOG.md`, then tags `v<version>`.
- `1.0.0` is the hand-written stable baseline. Do not move the baseline or tag a
  release by hand without maintainer approval.
- Bumping a bundled engine requires running the `freeze.yml` workflow **before**
  tagging (ADR 0001).
- `release.yml` builds four native installers into a draft. An operator verifies
  and publishes the draft.

See [`docs/cicd-process.md`](docs/cicd-process.md) for the full runbook.

## Documentation

- Update `docs/FEATURES.md` when a slice ships.
- Record decisions that pass the ADR three-test filter in `docs/adr/`.
- Track confirmed bugs in `docs/bugs.md`; CI/CD work in `docs/cicd-process.md`.
- Superpowers specs and plans are working files; delete them when the slice
  merges.

## Project layout

```
src/                  Frontend — UI only
src-tauri/src/        Rust core — commands, queue, scheduler, processors, history
scripts/binaries/     Sidecar manifest, lock, fetch, prep, audit
scripts/ci/           Freeze/unfreeze, package gate, workflow tests
docs/                 Architecture, features, ADRs, CI, release acceptance, bugs
```
