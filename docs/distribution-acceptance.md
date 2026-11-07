# Manual release acceptance

`node scripts/ci/package-gate.mjs --triple <triple>` proves the built package carries every engine where the app looks, that each engine starts, reports the manifest version, completes a real operation, and that the app's own resolver finds all five. It does not exercise the UI layer or every installer. Run this checklist against tagged release artifacts on real machines.

## Install

**macOS.** Builds are ad-hoc signed and not notarized (ADR 0002), so Gatekeeper quarantines the download. Copy first, then strip the mark recursively:

```bash
hdiutil attach ~/Downloads/*.dmg
cp -R /Volumes/<volume-name>/*.app /Applications/
hdiutil detach /Volumes/<volume-name>
xattr -dr com.apple.quarantine /Applications/<app-name>.app
```

The alternative is **System Settings → Privacy & Security → Open Anyway** after the first launch attempt.

**Windows.** Run the installer. When SmartScreen warns, choose **More info → Run anyway**. Builds are unsigned.

## UI checks

- Add files through the picker and drag & drop; remove one; clear the list.
- Change settings, close, reopen: values persist. Reset restores defaults.
- Compress a PNG, MP4, PDF and DOCX to their targets. Watch progress, cancel one file, then cancel a batch.
- Create and reveal the output folder. Confirm no original was modified.
- Open History: session and file records, filters, sort, pagination, detail changes/consent source, clear history.
- Trigger an error with an unsupported or corrupt file and confirm the message is actionable.
- Kanban: horizontal wheel/trackpad panning, thin scrollbar, drag ghost (item and lane), lane grip visibility, pinned-card focus, live compressing re-pin.
- Window: drag surface, minimize/maximize/close, theme toggle, single-instance focus, window-state restore.
- History table: trackpad scroll feel, sticky header, reveal actions.

The Windows WebView2 checks above need a real Windows machine. macOS trackpad and drag-overlay checks need a human pass; jsdom and WebKit synthesis cannot cover them.

## Acceptance matrix

One row per artifact. Record the version, date and machine.

| Platform | Artifact | PNG | MP4 | PDF | DOCX |
|---|---|---|---|---|---|
| macOS arm64 | DMG | | | | |
| macOS x64 | DMG | | | | |
| Windows x64 | NSIS | | | | |
| Windows x64 | MSI | | | | |
| Windows arm64 | NSIS | | | | |

Mark every row explicitly. Use **not run — no hardware** rather than leaving a cell blank or assuming green. The CI package gate opens the `.app` and the NSIS installer only; the MSI payload and the mounted DMG are manual checks.

Use real inputs: a PNG with metadata, an MP4 with audio, a text-bearing PDF, and a DOCX with an embedded image.

## Record failures

- Platform and exact artifact version.
- File type and input file.
- Exact error text the app showed.
- Whether the engine produced an output file; record its size and whether it opens.
- A screenshot is enough if it contains the error text.

Add confirmed defects to `docs/bugs.md` with the date. Do not record manual-check gaps as bugs; update this checklist instead.

## Automated half

```bash
node scripts/ci/package-gate.mjs --triple <triple>
```

Every release row runs it. Pass `--bundle-dir <path>` to point at a package outside `src-tauri/target/<triple>/release/bundle/`. It opens the macOS `.app` and the Windows NSIS installer payload; it does not open the `.dmg`, the `.app.tar.gz`, or the MSI.
