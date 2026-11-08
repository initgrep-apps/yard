# Updates are manual, signed, and draft-first

Yard checks for updates only when the user presses **Check for updates** in the About dialog. There is no background check, no scheduled poll, and no automatic download: the app contacts GitHub Releases only after a deliberate click. The check reads `latest.json` from `https://github.com/initgrep-apps/yard/releases/latest/download/latest.json`, offers the version, downloads it with progress, and installs with the Tauri updater plugin.

The artifact is signed with a project-owned Tauri updater key and the manifest carries that signature. `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` is the only key material in the repository; the private key and its password stay with the operator, offline. `requireSignedVersion: true` refuses a newer announced version paired with an older signed artifact. This signature is independent of OS code signing (ADR 0002): macOS remains ad-hoc signed and Windows unsigned, so the updater proves the artifact came from the release pipeline, not that Gatekeeper or SmartScreen trusts it.

Download and install are separate steps. A download may run during a compression batch; when it finishes while `isCompressing` is true, the state becomes `ready` and install/restart waits until the batch ends. Restarting mid-batch would kill active jobs. The provider owns the state machine; a top-right toast mirrors progress only while About is closed.

`latest.json` and the `.sig` assets are uploaded to the draft release, and `releases/latest` resolves only for a published, non-prerelease release. An installed app therefore cannot see an update before an operator publishes the draft. Existing 1.1.0 installs have no updater and need one manual installer; updates start with the next published release.

The network boundary is unchanged for files: the manifest fetch and the artifact download are the only requests, and neither carries file data, telemetry, or account information. Compression never touches the network.

## Considered Options

- **Manual check from About (chosen).** The user is already on the version row, the offline contract stays intact until they ask, and no new startup cost or surprise network traffic is introduced.
- **Automatic background check.** Rejected: it changes the product's offline promise and was not requested. It would also need a notification surface and a policy for metered connections.
- **Dialog-only, no toast.** Rejected: closing About during a long download would hide all progress, and the download survives the dialog by design.
- **Auto-install during a batch.** Rejected: installing restarts the app and would kill active compression jobs. Deferring to `ready` keeps both features usable.
- **Custom update server.** Rejected: GitHub Releases already hosts the artifacts, generates `latest.json`, serves `releases/latest`, and holds the signatures. A server adds an availability and credential surface with no gain.
- **OS code signing instead of Tauri signatures.** Not available: the project has no Apple Developer Program membership and no Authenticode certificate, and Tauri's updater requires its own signature regardless. The two signing layers are independent by design.

## Consequences

- Losing the private key or its password permanently stops updates for every existing install; the key cannot be rotated without a manual reinstall on each machine. Custody and offline backup are the recovery boundary. A leaked key alone does not allow an install, because release upload permission is also required.
- Every release must be built with `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `release.yml` runs `updater-preflight.mjs --require-secret` in the gate, and a missing key fails in seconds instead of after four 90-minute rows. Local installer builds need the same variables; the unit and frontend gates do not.
- `verify-updater` fails the release unless the manifest version matches the tag, all eight platform keys carry a URL and signature, every URL names an asset in the draft, and every referenced artifact has a matching `.sig` asset. The draft stays unpublished on failure.
- Drafts and prereleases never become `latest`, so publishing remains the moment an update becomes reachable. Manual acceptance of the update flow uses a separately published test patch, not the draft under review.
- The plugin builds with `native-tls` and the OS trust store rather than vendored OpenSSL, which keeps the `aarch64-pc-windows-msvc` cross-compile check free of an OpenSSL build.
- Every future bump of `tauri-plugin-updater` must keep `requireSignedVersion: true` and the signature verification path; the preflight and verifier enforce both in CI.
