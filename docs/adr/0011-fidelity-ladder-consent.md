# Fidelity ladder: cheapest size-reducing rung first, destructive only with consent

**Status:** accepted · supersedes ADR 0006

Attempts are ordered by how much they change the file, not by how convenient they are to try. Rung 0 copies an input already under target. Rung 1 is the cheapest rung that can reduce size: a lossless pass for PDF/zip (qpdf linearise, re-zip), a lossy encode for image and video. Further lossy attempts keep every page, track and frame in the same format and stay inside the quality floors; these lossless and floor-bounded lossy rungs run without asking because the user already authorised that range through the quality floors. Destructive changes — fewer pixels, a dropped stream or page, format conversion that discards data, rasterisation — run only with consent. If the target is still unreachable after every needed consent was granted, the smallest attempt is delivered as `best_effort`. A denied or timed-out rung is removed and never delivered; the file ends as `needs_permission` only when no authorised attempt remains. An earlier attempt whose changes were all authorised may still be delivered as `best_effort`, reporting its own changes and consent source. Silent best-effort delivery while a required change was never authorised is retired.

Two corrections are part of the same decision, because they are bugs rather than policy. First, when compression actually runs, EXIF orientation is applied to pixels and colour is transformed to sRGB with an sRGB profile attached: writing pixels while dropping the profile shifts colours, and keeping a Display-P3 or CMYK profile untransformed makes them look wrong in viewers that ignore profiles. An input already under target is copied byte-for-byte, so format conversion, orientation and colour normalization do not run: an under-target HEIC stays HEIC bytes under the mapped `.jpg` name, and an under-target image is not re-encoded to sRGB. Second, every output is validated for content equivalence — page count for PDF, stream count for video, entry set for archives — not just for a header and a non-zero size. Already-compressed archive entries are stored, not re-deflated.

## Considered Options

- **Consent-gated ladder (chosen).** The user stays in the loop for changes they can notice, while ordinary compression still runs unattended.
- **Silent best effort (ADR 0006).** Never tells the user that a track was dropped or a photo was downscaled; the audit trail cannot explain the result.
- **Ask for every lossy step.** A 50-photo batch becomes 50 prompts; the app is unusable at the default setting.
- **Refuse all lossy work by default.** Every video and JPEG target becomes impossible; the product does nothing out of the box.

## Consequences

- `best_effort` keeps its name but now means "every change the delivered attempt made was authorised; the target still did not fit". A denied or timed-out rung is removed; an earlier authorised attempt can still supply this result.
- New file state `needs_permission`, used only when no authorised attempt remains. The status enum and the TypeScript union change together.
- Video gains no lossless compression rung: remux changes the container, not the byte count. It is used for compatibility, not size.
- Outputs produced by compression are sRGB. A print or wide-gamut workflow is out of scope; converting is what keeps compressed results consistent across viewers. Under-target copies keep their original profile.
- HDR gain maps are a known limitation. The image and document saves use `keep=icc`, so an embedded gain map is dropped when compression runs. libvips 8.18.7 accepts `gainmap` in the keep flags but failed to re-save an Apple gain-map JPEG whose alternate-image ICC was absent, so enabling retention would turn working inputs into failed attempts. Retaining gain maps requires a fixture-backed check against the frozen vips build; tracked in `docs/bugs.md`.
- `qpdf --optimize-images --jpeg-quality=75` is reclassified as a lossy pass; it was previously presented as part of the safe part of the PDF ladder.
- The report for each file names the changes made and, when non-default, the consent source that permitted them.
