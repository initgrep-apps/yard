# macOS ships ad-hoc signed and un-notarized

We ship macOS builds ad-hoc signed (`bundle.macOS.signingIdentity = "-"`) with `com.apple.security.cs.disable-library-validation` granted through `src-tauri/Entitlements.plist`, and we do not notarize. There is no Apple Developer Program membership, and notarization requires one (Apple Developer Program, $99/year): Apple's notary service issues tickets only for software signed with a Developer ID certificate, which only the paid program provides.

## Considered Options

- **Entitlement route (chosen)** — keep Tauri's default hardened runtime and grant `com.apple.security.cs.disable-library-validation`. The app and its ad-hoc-signed sidecars can load the ad-hoc-signed dylibs they ship with; the hardened runtime stays on, so enabling notarization later only adds signing secrets and their env wiring.
- **`hardenedRuntime: false`** — it works today and would let the helpers load their dylibs. Rejected: Apple requires the hardened runtime for notarization, so the flag would have to be flipped back on before a membership is purchased. A forgotten flag is a silent, expensive failure later.
- **Ship only notarized builds** — requires an Apple Developer Program membership that does not exist.

## Ruling (2026-10-21): entitlement, not a disabled hardened runtime

`src-tauri/Entitlements.plist` grants exactly one entitlement, `com.apple.security.cs.disable-library-validation = true`, and `bundle.macOS.entitlements` points at it. Tauri passes the entitlements to the app and its `externalBin` sidecars while signing the executables with `--options runtime`. The framework dylibs are signed ad-hoc without the hardened runtime; the entitlement carried by the executables is what lets them load those dylibs.

The defect this fixes: ad-hoc signing plus the hardened runtime made macOS library validation reject the ad-hoc-signed dylibs the helpers ship with ("mapping process and mapped file have different Team IDs"). In the `v0.1.0-alpha.1` release build (an unpublished draft), `vips`, `qpdf` and `gs` died with `Library not loaded`, so image, PDF and document compression failed. `ffmpeg`/`ffprobe` carry no bundled dylibs — they link only Apple system frameworks — so they kept working.

The entitlement is verified at two levels: a Rust test parses the plist and asserts exactly one top-level boolean-true entitlement while `hardenedRuntime` is not explicitly false, and the release package gate runs every helper from the built `.app` (a helper that cannot start fails the row).

## What changes when an Apple Developer Program membership is added

1. Apple issues a **Developer ID Application** certificate and a Team ID.
2. Export the certificate as a `.p12`, base64 it, and store it as a GitHub secret.
3. Add to both macOS rows of `release.yml`: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, plus `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID` — or the App Store Connect API key form: `APPLE_API_KEY` (the Key ID), `APPLE_API_ISSUER`, and `APPLE_API_KEY_PATH` (the `.p8` file).
4. Tauri resolves `APPLE_SIGNING_IDENTITY` before `bundle.macOS.signingIdentity`, so the environment overrides the pinned `"-"`; no config edit is required. Tauri signs with the real certificate and submits the app to Apple's notary automatically. No Rust or workflow logic changes — only the signing secrets and the env wiring from step 3. The hardened runtime stays on; every dylib is signed with the same Developer ID, so the entitlement becomes unnecessary and may be deleted.

## Consequences

- Until a membership exists, Gatekeeper does not trust the download and macOS installs need a documented quarantine removal after copying the app (see `docs/cicd-process.md` §5).
- The entitlement is a mild security relaxation: the app may load libraries signed by another team. The app loads no pluggable code and only the 71 dylibs it ships plus Apple's system frameworks.
- Windows builds are unaffected by this ADR; they remain unsigned and show the SmartScreen "More info → Run anyway" flow.
