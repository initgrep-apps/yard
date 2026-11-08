# Security policy

Yard processes files locally and sends no file contents or telemetry anywhere.
The only network requests are the update check and download the user starts from
**About**, over HTTPS to GitHub Releases. The main risks are in the bundled
compression engines and the release supply chain, so reports about either are
welcome.

## Supported versions

| Version | Supported |
|---|---|
| 1.x | Yes |
| 0.x (including prereleases) | No — upgrade to 1.x |

Security fixes ship in the latest 1.x release. Older 1.x builds are not patched
separately.

## Reporting a vulnerability

**Do not open a public issue.**

Use GitHub private vulnerability reporting:

<https://github.com/initgrep-apps/yard/security/advisories/new>

Include:

- the affected version and platform (macOS arm64/Intel, Windows x64/arm64),
- steps to reproduce, or a proof of concept,
- the impact you believe it has,
- any suggested fix or mitigation.

If the report involves a bundled engine (libvips, FFmpeg, qpdf, Ghostscript) and
the issue is reachable through Yard, report it here first. We will coordinate
with upstream if needed.

## What to expect

- **Acknowledgement** within 3 business days.
- **Initial assessment** (severity, affected versions, next steps) within 10
  business days.
- **Fix** targeted within 30 days for high-severity issues and 90 days for
  lower-severity issues, depending on complexity and upstream releases.
- **Coordinated disclosure**: we will agree on a public disclosure date with
  you, normally after a fixed release is available. We credit reporters unless
  asked not to.

This is a volunteer-maintained project. There is no bug bounty.

## Scope

In scope:

- the Yard application and its Rust core,
- the bundled engine binaries and how they are staged, verified, and packaged,
- the build and release workflows in `.github/workflows/` and `scripts/`.

Out of scope:

- warnings caused by the current lack of code signing and notarization,
- vulnerabilities in third-party engines that are not reachable through Yard,
- files supplied by the user,
- social engineering and physical access attacks.

## Release integrity

Release assets are built on native GitHub-hosted runners after the full test
suite and package-content gate pass. Each installer has a SHA-256 checksum on
the release page, alongside an SBOM and third-party notices. Builds are not
code-signed or notarized yet; verify checksums before installing.

In-app updates carry a second, independent check. Artifacts are signed with the
project's Tauri updater key, and the `latest.json` manifest carries each
artifact's signature. The app verifies the signature before installing and
refuses an artifact whose announced version differs from the signed version
(`requireSignedVersion`), which blocks a crafted response that pairs a newer
version with an older signed build. This signature proves the artifact came from
the release pipeline; it is separate from OS code signing, so it does not make
Gatekeeper or SmartScreen trust the download.

The updater private key and its password are operator-owned and backed up
offline. Losing either permanently stops updates for every existing install,
because the key cannot be rotated without a manual reinstall. A leaked key alone
does not let an attacker install anything: release upload permission is also
required.

`latest.json` and the `.sig` assets are uploaded to a draft release. Drafts and
prereleases are invisible to `releases/latest`, so an update becomes reachable
only after an operator publishes. Existing 1.1.0 installs have no updater, so the
first updater-enabled release must be installed manually.
