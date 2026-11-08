# Manual signed in-app updates

**Status:** proposed
**Date:** 2026-11-08
**Scope:** Tauri updater plugin, About dialog update flow, update signing, GitHub release manifest, CI verification

## Summary

Add a manual update check to the About dialog. The user clicks **Check for updates**; the app reads a signed `latest.json` from the latest published GitHub release, offers an update, downloads it with progress, and restarts into the new version. There is no background check and no automatic download.

The update flow lives in the About dialog. A top-right toast is used only when the dialog is closed during a download or while an update is ready. Downloads never interrupt an active compression batch; installation and restart are deferred until the batch finishes.

All update messages are generic: they say “the app”, never “Yard”. The About dialog’s brand title remains “Yard”.

The updater uses Tauri’s own signature, separate from Apple or Windows code signing. The keypair and GitHub secret setup are manual operator tasks, listed below with exact commands.

## Goals

- Manual-only update check from About.
- Clear version, status, progress, and error states in one place.
- Download may run during a compression batch; install/restart may not.
- Signed update artifacts and a signed `latest.json`; the app verifies before install.
- Draft-first release flow: updates become visible only after the draft is published.
- Keep both macOS DMG arches and both Windows MSI and NSIS installers.
- Preserve the existing claim that files never leave the machine.

## Non-goals

- Automatic or scheduled update checks.
- Delta updates; every update downloads the full artifact.
- Linux packages.
- Removing MSI or NSIS.
- Apple notarization or Windows Authenticode signing.
- A custom update server; GitHub Releases is the CDN.

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Trigger | Manual **Check for updates** in About | Keeps the app offline unless the user asks for network access |
| Primary surface | About dialog | The user is already there and sees the version |
| Fallback surface | Top-right toast | Keeps progress visible if About is closed; no duplicate while About is open |
| Download/install | Separate steps | Allows downloading during a batch and deferring restart |
| Batch conflict | Defer install until `isCompressing` is false | A restart would kill active jobs |
| Windows formats | Keep MSI and NSIS | Both install types are supported; updater selects by installer key |
| Signing | Tauri updater keypair | Mandatory signature verification; independent of OS code signing |
| Manifest | `latest.json` in the GitHub release | No server; tauri-action generates it |
| Version security | `requireSignedVersion: true` | Prevents pairing a newer announced version with an older signed artifact |
| Release visibility | Draft until verified and accepted | Existing apps only see published, non-prerelease releases |

## UX flow

### Layout

The existing About dialog keeps its icon, title, description, license rows, and third-party notices. The version line becomes a row:

```text
        [icon]
         Yard
Offline file compression tool

┌──────────────────────────────────────────┐
│ Version 1.1.0          [ Check for updates ] │
│ You're on the latest version.                │
└──────────────────────────────────────────┘

[update card appears here only when relevant]

──────────────────────────────────────────
Yard                  GPL-3.0
FFmpeg / ffprobe      GPL-2.0-or-later
Ghostscript            AGPL-3.0
libvips                LGPL-3.0
qpdf                   Apache-2.0

[Third-party notices]
```

Use the existing shadcn/base components:

- `Item` for the version row and update card.
- `Button` variants for actions.
- `Progress` for download progress.
- `Badge` for “available”, “up to date”, and “ready” status.
- `Separator` between the update area and the license list.
- `Alert` for errors. Add `alert` from the shadcn registry because it is not installed yet.
- `Spinner` for checking and installing.
- Icons from Lucide, passed as components and marked with `data-icon="inline-start"`.

### State table

| State | About dialog | Toast when About is closed |
|---|---|---|
| Idle | `Version 1.1.0` + **Check for updates** | none |
| Checking | Button disabled, `Spinner`, “Checking for updates…” | none |
| Current | “You're on the latest version.” + **Check again** | none |
| Available | Card: “Version 1.2.0 is available”, release date, **What's new**, **Update now**, **Not now** | none |
| Downloading | Progress bar, “42% · 12.4 MB of 96.2 MB”, “You can close this window. The download continues in the background.” | Top-right progress toast |
| Ready | “Update ready.” + **Restart app** | Top-right ready toast |
| Ready, batch active | “Update ready.”, “Finish the current batch to restart.”, **Restart app** disabled | Top-right ready toast with the same explanation |
| Installing | Progress at 100%, “Installing update…”, “The app will restart.” | Installing toast |
| Error | `Alert` with the reason, **Try again**, **Download from GitHub** | Error toast with **Retry**, top-right |
| Installed, relaunch failed | “The update was installed. Restart the app to finish.” + **Try again** | Error toast with **Retry** |

### State machine

```text
idle
  └─ check() ──> checking
                   ├─ no update ──> current
                   ├─ update ─────> available
                   └─ failure ────> error

available
  ├─ Update now ──> downloading
  └─ Not now ─────> idle

downloading
  ├─ finished, not compressing ──> installing
  ├─ finished, compressing ──────> ready
  └─ failure ────────────────────> error

ready
  ├─ Restart app, not compressing ──> installing
  └─ Restart app, compressing ──────> ready (disabled)

installing
  ├─ macOS: install() then relaunch()
  ├─ Windows: install() exits the app; the installer restarts it
  ├─ install failure ──> error
  └─ install success, relaunch failure ──> error (relaunch stage)

error
  ├─ Try again ──> checking | downloading | ready (retry the failed stage)
  └─ Download from GitHub ──> opens the latest release page
```

### UI copy

All strings are generic and avoid the app name.

| Context | String |
|---|---|
| Version row | `Version {version}` |
| Idle action | `Check for updates` |
| Checking | `Checking for updates…` |
| Current | `You're on the latest version.` |
| Current action | `Check again` |
| Available title | `Version {version} is available` |
| Available meta | `Released {date}.` |
| Release notes link | `What's new` |
| Available actions | `Update now` / `Not now` |
| Restart note | `The app will restart after installing. Staged files that haven't run will be cleared.` |
| Downloading | `Downloading update…` |
| Download progress | `{percent}% · {downloaded} of {total}` |
| Download note | `You can close this window. The download continues in the background.` |
| Ready | `Update ready.` |
| Ready description | `Restart the app to finish installing version {version}.` |
| Ready while compressing | `Finish the current batch to restart.` |
| Ready action | `Restart app` |
| Installing | `Installing update…` |
| Installing description | `The app will restart.` |
| Check failure | `Couldn't check for updates. Check your connection and try again.` |
| Download failure | `The update couldn't be downloaded. Check your connection and try again.` |
| Install failure | `The update couldn't be installed. Try again, or download the latest installer from GitHub.` |
| Install permission/path failure (macOS) | `The app couldn't be replaced. Move it to Applications, then try again.` |
| Relaunch failure after install | `The update was installed. Restart the app to finish.` |
| Retry action | `Try again` |
| GitHub fallback | `Download from GitHub` |
| Dev build | `Updates are disabled in development builds.` |
| Toast: downloading | Title `Downloading update`, description `Version {version} · {percent}%` |
| Toast: ready | Title `Update ready`, description `Restart the app to finish.` |
| Toast: installing | Title `Installing update`, description `The app will restart.` |
| Toast: error | Title `Update failed`, action `Retry` |

### About dialog behavior

- The update row is rendered even when no update has been checked.
- The version comes from `package.json`, matching the existing About behavior.
- The check button is disabled in development builds with the dev-build message.
- “Not now” closes the update resource and returns to idle.
- “What's new” opens the GitHub release page for the announced version.
- The dialog may be closed at any time. The provider owns update state, so closing does not cancel a download.
- The update card uses `Item variant="outline"` and grows the dialog; the existing `max-h-[90dvh] overflow-y-auto` handles small windows.
- Status changes use `aria-live="polite"`; progress percentage is not announced on every chunk.

### Toast behavior

The project uses the shadcn Sonner wrapper at `src/components/ui/sonner.tsx`. The update toast is a Sonner custom toast rendered through that `Toaster`.

- `toast.custom` with a stable id, `position: "top-right"`, `duration: Infinity`.
- The `Toaster` offset is set so top-right toasts sit below the 48 px title bar, not over the window controls:
  `offset={{ top: 56, bottom: 16, left: 16, right: 16 }}`.
- The toast renders a small card with an icon, title, description, and `Progress`.
- Progress updates are throttled to roughly four per second.
- The toast is shown only while About is closed. Opening About dismisses it and shows the inline state.
- The toast is dismissed when the state returns to idle or current.
- Errors replace the progress toast with a standard `toast.error` carrying a **Retry** action.
- The toast has no cancel action; the plugin does not expose download cancellation.

### Batch interaction

The provider reads `isCompressing` from `CompressionContext`.

- Download starts even while a batch is running.
- When download finishes with `isCompressing === false`, install immediately.
- When download finishes with `isCompressing === true`, enter `ready` and do not install.
- **Restart app** is disabled while compressing, with the “Finish the current batch to restart.” description.
- Starting a batch during download is allowed; the same deferral applies at completion.
- The update card explains that a restart clears staged files that have not run.

## Architecture

### Frontend

New units:

- `src/providers/UpdateProvider.tsx`
  - Owns the state machine.
  - Reads `isCompressing` from `CompressionContext`.
  - Exposes `state`, `check`, `startDownload`, `install`, `decline`, `retry`, `setSurfaceOpen`.
  - Manages the update resource lifecycle: `update.close()` when declining or replacing a found update.
  - Owns toast effects.
  - Uses `usePlatform()` for macOS relaunch vs Windows exit behavior.
  - Uses `check({ timeout: 15_000 })` and `download(..., { timeout: 600_000 })`.
- `src/components/update/UpdateSection.tsx`
  - Renders the version row and each state inside About.
- `src/components/update/UpdateToast.tsx`
  - Custom toast card with `Progress`.
- `src/components/AboutDialog.tsx`
  - Replaces the bare version line with `UpdateSection`.
  - Calls `setSurfaceOpen(open)` so the provider knows when to toast.
- `src/App.tsx`
  - Wraps the shell in `UpdateProvider`, inside `CompressionProvider`.
  - Passes the toast offset to `Toaster`.

Provider placement:

```tsx
<CompressionProvider>
  <QuitGuard />
  <UpdateProvider>
    <HistoryProvider>
      <ShellFrame />
    </HistoryProvider>
  </UpdateProvider>
</CompressionProvider>
```

This placement lets the provider read `isCompressing` and keeps update state alive across dialog open/close.

### State type

```ts
type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "current" }
  | { status: "available"; update: Update }
  | { status: "downloading"; update: Update; downloaded: number; total: number | null }
  | { status: "ready"; update: Update }
  | { status: "installing"; update: Update }
  | { status: "error"; stage: "check" | "download" | "install"; message: string; update: Update | null }
```

### Tauri backend

- Add `tauri-plugin-updater` and `tauri-plugin-process` to `src-tauri/Cargo.toml`.
- Register both in `src-tauri/src/lib.rs`.
- Add `updater:default` and `process:default` to `src-tauri/capabilities/default.json`.
- Add `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process` to `package.json`.

### Configuration

`src-tauri/tauri.conf.json`:

```json
"bundle": {
  "createUpdaterArtifacts": true
},
"plugins": {
  "updater": {
    "pubkey": "REPLACE_WITH_UPDATER_PUBLIC_KEY",
    "endpoints": [
      "https://github.com/initgrep-apps/yard/releases/latest/download/latest.json"
    ],
    "requireSignedVersion": true,
    "windows": {
      "installMode": "passive"
    }
  }
}
```

The public key is filled in by the operator. The implementation must not generate or commit a private key.

## Signing and operator tasks

These are manual tasks. The implementation provides instructions and validation, but does not generate keys or set secrets.

### 1. Generate the updater keypair

```bash
yarn tauri signer generate -w ~/.tauri/yard-updater.key
```

The command creates:

- `~/.tauri/yard-updater.key` — private key. Never commit or paste it into chat.
- `~/.tauri/yard-updater.key.pub` — public key. Safe to commit.

Use a password when prompted.

### 2. Back up the private key and password

Store both offline. Losing either means existing installs can never update again. This key cannot be rotated without a manual reinstall on every machine.

### 3. Add GitHub Actions secrets

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/yard-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
gh secret list | grep TAURI_SIGNING
```

The second command prompts for the password. The private key secret contains the file contents, not a path.

### 4. Put the public key in the config

Paste the contents of `~/.tauri/yard-updater.key.pub` into `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`, replacing the placeholder.

### 5. Local installer builds

Signed local installers need the same environment variables:

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/yard-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="..."
yarn tauri build
```

Local builds without the key still run the frontend gate and tests. Building installers locally requires the key.

### 6. Release preflight

`scripts/ci/updater-preflight.mjs` fails with actionable instructions when:

- `bundle.createUpdaterArtifacts` is not `true`;
- the public key is empty or still the placeholder;
- the endpoint is missing or not the expected GitHub URL;
- `requireSignedVersion` is not `true`;
- the signing secret is missing in the release gate.

## CI/CD changes

### Release gate

- Run the updater preflight before the draft is reused or created.
- The preflight checks the signing secret is present, but never prints it.

### Release rows

Pass the signing secrets to `tauri-action`:

```yaml
env:
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

Set:

```yaml
with:
  releaseId: ${{ needs.gate.outputs.release_id }}
  releaseDraft: true
  retryAttempts: 3
  updaterJsonPreferNsis: true
  args: --target ${{ matrix.triple }}
```

- `tauri-action` uploads the `.sig` files and generates `latest.json` by default.
- The pinned action includes the May 2026 jitter fix for the parallel `latest.json` race; `retryAttempts: 3` is a second layer.
- Both MSI and NSIS are signed. The updater looks up `windows-{arch}-msi` or `windows-{arch}-nsis` first, then falls back to `windows-{arch}`.
- `updaterJsonPreferNsis: true` makes the generic Windows entry point at NSIS, matching the README download link.

### New verification job

Add a final job after all four release rows:

- Runs on `ubuntu-24.04`, `contents: read`.
- Downloads `latest.json` from the draft release.
- Requires `version` to equal the tag version.
- Requires platform entries for:
  - `darwin-aarch64`
  - `darwin-x86_64`
  - `windows-aarch64`
  - `windows-x86_64`
  - `windows-aarch64-nsis`
  - `windows-aarch64-msi`
  - `windows-x86_64-nsis`
  - `windows-x86_64-msi`
- Requires non-empty `signature` and `url` for each entry.
- Requires every URL to point at an asset present in the draft.
- Requires the six signature assets:
  - `yard_<version>_aarch64.app.tar.gz.sig`
  - `yard_<version>_x64.app.tar.gz.sig`
  - `yard_<version>_arm64-setup.exe.sig`
  - `yard_<version>_x64-setup.exe.sig`
  - `yard_<version>_arm64_en-US.msi.sig`
  - `yard_<version>_x64_en-US.msi.sig`
- Fails without publishing. The draft stays for inspection.

The verification logic lives in `scripts/ci/verify-updater-manifest.mjs`, with fixture tests.

### Draft-first behavior

- `latest.json` is uploaded to the draft, so existing apps cannot see it.
- `https://github.com/.../releases/latest/download/latest.json` resolves only after the release is published.
- Prereleases do not become `latest`.
- Publishing remains the existing manual step after all rows are green and acceptance passes.

## Security and offline behavior

- The update manifest is fetched over HTTPS from GitHub.
- The artifact signature is mandatory; `requireSignedVersion` prevents downgrade pairing.
- The updater checks for a newer version by default; downgrades remain disabled.
- The app sends no file data, telemetry, or account information. The only new network request is the manual update check and the download the user starts.
- README, Security, and Architecture copy must change from “no network access” to “files never leave the machine; the app only contacts GitHub when the user checks for updates.”
- The private key is the recovery boundary. If it leaks, an attacker still needs release upload permission; if it is lost, updates stop.
- macOS builds remain ad-hoc signed and not notarized. The updater signature is independent, but Gatekeeper, translocation, and permission failures must be tested on real hardware.
- macOS app translocation (running from Downloads) can make the bundle path read-only; the install error must tell the user to move the app to Applications.
- Windows MSI updates may prompt for UAC; NSIS per-user updates should not.

## Testing

### Automated

- `UpdateProvider` tests:
  - idle → checking → current;
  - idle → checking → available;
  - available → downloading → ready;
  - ready → installing → relaunch on macOS;
  - Windows install path does not call `relaunch`;
  - download during a batch produces `ready`, not `installing`;
  - restart is disabled while `isCompressing`;
  - check/download/install errors map to the correct messages;
  - “Not now” calls `update.close()`;
  - toast appears only when About is closed;
  - progress is throttled.
- `AboutDialog` tests for every state and all copy.
- Assert no update check on mount; checks happen only from the button.
- Assert dev builds disable the button.
- `updater-preflight.mjs` tests: valid config, placeholder key, missing endpoint, wrong `requireSignedVersion`, missing secret.
- `verify-updater-manifest.mjs` tests: valid manifest, missing platform, empty signature, URL not in release, wrong version.
- Existing test suite and cargo tests remain green.

### Manual acceptance

Add to `docs/cicd-process.md` §5:

- Check with no update available.
- Check offline or with GitHub blocked; error is actionable.
- Publish a test patch; update from the previous installed version on:
  - macOS arm64;
  - macOS x64;
  - Windows x64 NSIS;
  - Windows x64 MSI;
  - Windows arm64 NSIS;
  - Windows arm64 MSI.
- Start a batch during download; update reaches Ready and does not restart until the batch ends.
- Close About during download; toast continues; reopening About shows inline progress.
- Verify History, settings, and output folder survive the update.
- Verify a tampered signature is rejected and the app stays on the old version.
- Verify staged files that had not run are cleared after restart, as stated.

## Rollout

1. Implement and test the feature.
2. Operator generates the keypair, sets secrets, and fills the public key.
3. Release the first updater-enabled version. Existing installs must install it manually because they have no updater.
4. Publish a second patch and verify the in-app update on all supported install types.
5. Keep the private key and password backed up for the life of the app.

## Documentation updates

- `README.md` — update the offline/network wording and mention manual updates.
- `docs/ARCHITECTURE.md` — add the update subsystem, network boundary, and key custody.
- `docs/FEATURES.md` — add a “Manual update check” row.
- `docs/cicd-process.md` — add signing, `.sig`, `latest.json`, and the verification job to the release diagram and manual acceptance.
- `CONTRIBUTING.md` — note the signing key requirement for local installer builds.
- `AGENTS.md` — update the release workflow bullet.
- `docs/adr/0015-manual-signed-updates.md` — record the decision, the manual-check choice, the signing boundary, and the rejected alternatives.

## Rejected alternatives

- **Automatic background check.** Rejected for now: it changes the offline contract and was not requested.
- **Dialog-only, no toast.** Rejected: closing About during a long download would hide all progress.
- **Auto-install even during a batch.** Rejected: it would kill active jobs.
- **Custom update server.** Rejected: GitHub Releases already provides hosting, `latest.json`, and signatures.
- **Removing MSI.** Rejected by product choice; both Windows install types stay.
- **Using OS code signing instead of Tauri signatures.** Rejected: Tauri’s updater requires its own signature and Apple/Windows signing is not available for this project.
