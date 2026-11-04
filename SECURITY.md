# Security policy

Yard runs entirely offline and does not send file contents or telemetry anywhere.
The main risks are in the bundled compression engines and the release supply chain,
so reports about either are welcome.

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
