# Open bugs

Confirmed defects only. Accepted limitations live in `docs/adr/` and `docs/ARCHITECTURE.md`; manual release checks and CI work in `docs/cicd-process.md`.

| Bug | Impact | Next step |
|---|---|---|
| HDR gain maps are stripped when an image or document media entry is recompressed | HDR photos lose gain-map data. Saves use `keep=icc` only. | Enable the libvips `gainmap` keep flag after a fixture proves the frozen vips build round-trips an Apple gain-map JPEG. |

No other confirmed open bugs.
