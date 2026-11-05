# Fidelity ladder: lossless first, lossy next, destructive only with consent

**Status:** accepted · supersedes ADR 0006

Attempts are ordered by how much they change the file, not by how convenient they are to try: (0) copy when already under target, (1) lossless pass — remux, re-zip, qpdf linearise, (2) lossy re-encode that keeps every page, track, frame and the same format, (3) destructive change — fewer pixels, dropped stream or page, format conversion that discards data, rasterisation, (4) target unreachable. Rungs 0–2 run without asking because the user already authorised that range through the quality and resolution floors. Rung 3 runs only with consent. If the target is still unreachable after every needed consent was granted, the smallest attempt is delivered as `best_effort`; if consent was denied or timed out, the file ends as `needs_permission` and nothing is delivered. Silent best-effort delivery while a required change was never authorised is retired.

Two corrections are part of the same decision, because they are bugs rather than policy. First, EXIF orientation is always applied to pixels, and colour is transformed to sRGB with an sRGB profile attached: writing pixels while dropping the profile shifts colours, and keeping a Display-P3 or CMYK profile untransformed makes them look wrong in viewers that ignore profiles. Second, every output is validated for content equivalence — page count for PDF, stream count for video, entry set for archives — not just for a header and a non-zero size. Already-compressed archive entries are stored, not re-deflated.

## Considered Options

- **Consent-gated ladder (chosen).** The user stays in the loop for changes they can notice, while ordinary compression still runs unattended.
- **Silent best effort (ADR 0006).** Never tells the user that a track was dropped or a photo was downscaled; the audit trail cannot explain the result.
- **Ask for every lossy step.** A 50-photo batch becomes 50 prompts; the app is unusable at the default setting.
- **Refuse all lossy work by default.** Every video and JPEG target becomes impossible; the product does nothing out of the box.

## Consequences

- `best_effort` keeps its name but now means "every required change was authorised; the target still did not fit".
- New file state `needs_permission`. The status enum and the TypeScript union change together.
- Video gains no lossless compression rung: remux changes the container, not the byte count. It is used for compatibility, not size.
- Outputs are sRGB. A print or wide-gamut workflow is out of scope; converting is what keeps the result consistent across viewers.
- HDR gain maps embedded in images are bulky metadata, but removing one changes how an HDR photo displays. Treated as a destructive change, not a silent strip.
- `qpdf --optimize-images --jpeg-quality=75` is reclassified as a lossy pass; it was previously presented as part of the safe part of the PDF ladder.
- The report for each file names the changes made and the consent that permitted them.
