# Changelog

## Unreleased

### Added

- **Memory budget** — batches are admitted against `clamp(total RAM / 8, 300 MiB,
  2 GiB)` instead of a slot count. Settings → Performance exposes adaptive memory
  and a fixed ceiling; `memory_stats` and `memory:usage` report budget and RSS.
- **Consent gate** — destructive changes (downscale, dropped stream, format change,
  rasterise) are checked before delivery and ask when the matrix allows consent.
  Inline prompts grant once or for the batch; the pre-flight dialog covers HEIC → JPEG
  when its default permission is off. A denied or unanswered rung is removed; the file
  ends as `needs_permission` only when no authorised attempt remains. An earlier
  authorised over-target attempt may still be delivered as best effort.
- **History truth** — file records carry `changes` and `consentSource`; the batch
  outcome counts `needs_permission`.
- **Image long-edge floor** — `qualityFloors.image.minLongEdge` (default 1280 px)
  bounds the dimension ladder.

### Changed

- **Documents stream** — archive entries are copied one at a time instead of
  snapshotting the whole archive. Temp disk grows; RAM no longer does.
- **Under-target copies** — an input already under target is copied byte-for-byte under
  the mapped extension. HEIC/MOV bytes and the original image colour/orientation remain
  until compression actually runs.
- **Images** — every compression attempt uses `vips thumbnail` with EXIF autorotation,
  sRGB output and `keep=icc`. Metadata is no longer blanket-stripped.
- **Video** — one ABR rung then CRF at the lower allowed resolutions; all audio/subtitle
  tracks and chapters are mapped; output is `yuv420p` with `+faststart`; duration and
  stream counts are validated.
- **PDF** — page counts are compared through qpdf; the qpdf image pass is lossy;
  every gs grid pass carries `-dPDFSETTINGS=/screen`. The stale "2 qpdf + 17 gs"
  bound is now "2 qpdf + up to 16 gs cells, plus page probes".
- **One file-type table** — `registry.rs` drives detection, accepted extensions
  and output naming.
- **Job bodies run in `spawn_blocking`** — engine polling no longer parks tokio
  runtime workers; release builds keep `panic = "unwind"` so one file's panic is
  contained.

### Fixed

- Portrait photos keep their orientation, and non-sRGB inputs are transformed and
  tagged sRGB instead of losing their profile.
- Cancelled or failed copies leave no partial file in the output folder.
- PDFs that lose pages are rejected instead of reported as done.
- Video no longer silently drops extra audio or subtitle tracks.
- A processor panic no longer kills the app in a release build.
- Sidecar stderr and stdout drains are bounded and joined.

## [1.3.1](https://github.com/initgrep-apps/yard/compare/v1.3.0...v1.3.1) (2026-11-08)


### Bug Fixes

* restore logical window geometry and kanban lane layout ([#244](https://github.com/initgrep-apps/yard/issues/244)) ([fc87370](https://github.com/initgrep-apps/yard/commit/fc873709994eb325da2962e98a14d45681b9c41b))

## [1.3.0](https://github.com/initgrep-apps/yard/compare/v1.2.0...v1.3.0) (2026-11-08)


### Features

* **audit:** add a manifest-driven node gs resolver for the audit smoke ([f3b4e1c](https://github.com/initgrep-apps/yard/commit/f3b4e1c4151db3594730b443948469497ca5e065))
* **binaries:** add qpdf/vips/gs manifest rows and stage qpdf lib dir in fetch ([f6a6da0](https://github.com/initgrep-apps/yard/commit/f6a6da05d280a5c0d4e62e2ecf646d8850392f7b))
* **binaries:** engines gain extraPackages rows on url entries plus lock-backed staging digests ([1dd5e9e](https://github.com/initgrep-apps/yard/commit/1dd5e9e9b789a683fb0488d90bf9d2232152b5aa))
* **binaries:** fetch stages gs extra packages with per-member digest lock rows ([e087b77](https://github.com/initgrep-apps/yard/commit/e087b77e752e073f84b6e1e987c70672c3335c8b))
* **binaries:** gate gs resources in the audit plus a brew-free clean-room smoke ([9f9a393](https://github.com/initgrep-apps/yard/commit/9f9a393835b08671337846091b7cea40ed574769))
* **binaries:** macOS arm64 prep — official qpdf, brew vips/gs, staged gs resources ([5f28667](https://github.com/initgrep-apps/yard/commit/5f28667b961e8a6b73985878495a7d23c2748995))
* **binaries:** macOS Intel target manifest (Riedl amd64, qpdf x86_64) ([9392b62](https://github.com/initgrep-apps/yard/commit/9392b6232d442b67a9a1cc79e8507a025e33ddc1))
* **binaries:** macOS Intel target manifest (Riedl amd64, qpdf x86_64) ([d22e9bf](https://github.com/initgrep-apps/yard/commit/d22e9bfadcc328c801cdb6a0a87064eefe23f84c))
* **binaries:** manifest-driven fetch with arch gate; fix Intel ffmpeg mislabel ([708276e](https://github.com/initgrep-apps/yard/commit/708276ea08b257f4fd276dd040d6f4f3507b0306))
* **binaries:** manifest-driven fetch with arch gate; fix Intel ffmpeg mislabel ([7667336](https://github.com/initgrep-apps/yard/commit/7667336c0e05eaf143809314546dbe711541ac8e))
* **binaries:** pin the full windows gs dependency closure as 25+23 extraPackages rows ([3eb8205](https://github.com/initgrep-apps/yard/commit/3eb8205813bb77272c146385b881b78b401f5d83))
* **binaries:** pure-node PE import table parser for windows engine closure checks ([409c78f](https://github.com/initgrep-apps/yard/commit/409c78f834dd98abdb69a545a8d740da8c6966d1))
* **binaries:** real macOS prep — brew vips/gs staging, official qpdf lib, gs resources ([f6affd1](https://github.com/initgrep-apps/yard/commit/f6affd1a67efedd19a96d6c480ec64b1efe73787))
* **binaries:** refresh flow for vendor version bumps ([e473790](https://github.com/initgrep-apps/yard/commit/e47379042b57f71c3bd11737a032692c3201abec))
* **binaries:** refresh flow for vendor version bumps ([cc37aa3](https://github.com/initgrep-apps/yard/commit/cc37aa37f7ff8deeec5a49e3b7583ed41ca75548))
* **binaries:** SBOM + third-party license notices from the lock ([8629b38](https://github.com/initgrep-apps/yard/commit/8629b3845c7c2d18932a8339ccbeb19c4ffca461))
* **binaries:** SBOM + third-party license notices from the lock ([98a1951](https://github.com/initgrep-apps/yard/commit/98a1951fda0f94eb72908a91578967568f41d285))
* **binaries:** unfreeze writes a restore provenance marker; verify skips stagedSha256-vs-lock when it matches ([3d4a769](https://github.com/initgrep-apps/yard/commit/3d4a769dacf632f62fe59028a48905999fcef05c))
* **binaries:** windows engine import-closure check wired into windows prep and audit ([6af8e67](https://github.com/initgrep-apps/yard/commit/6af8e67aeb9ba2e7f2acba3fa3d0838c371673b0))
* **binaries:** windows prep stages extra package DLLs beside engines guarded by lock pins ([7801bc4](https://github.com/initgrep-apps/yard/commit/7801bc4d1fca246fb3ca43682e06bd549a428f55))
* **binaries:** Windows x64 + arm64 pipeline (BtbN/qpdf/MXE/MSYS2, engine folders) ([e28ee07](https://github.com/initgrep-apps/yard/commit/e28ee0714acf6e31fe6127e9894f8349cfa60bd6))
* **binaries:** Windows x64 + arm64 pipeline (BtbN/qpdf/MXE/MSYS2, engine folders) ([4fd9a49](https://github.com/initgrep-apps/yard/commit/4fd9a49abae3c24dfa56b2545d18923946cf3765))
* bounded memory, consent-gated fidelity, and backend remediation ([#224](https://github.com/initgrep-apps/yard/issues/224)) ([602c283](https://github.com/initgrep-apps/yard/commit/602c2831176f2df78d3f344ff11cff905f4618f1))
* **ci:** accept semver prerelease tags (alpha/beta) in the release version check ([a4833b1](https://github.com/initgrep-apps/yard/commit/a4833b1937e8609482bf8e014b83dd4c3cb0eaf1))
* **ci:** gate the release on the built package working ([#147](https://github.com/initgrep-apps/yard/issues/147)) ([03e1f7d](https://github.com/initgrep-apps/yard/commit/03e1f7d43320dc8b60edb67419e29b6eb0d7edd1))
* **ci:** PR gate workflow + docs (Task 4) ([9009a0c](https://github.com/initgrep-apps/yard/commit/9009a0ce2cbdd383d40cc89445c5134a55ac67a1))
* **compress:** add pure grouping/counts/filter lib for run board ([b809beb](https://github.com/initgrep-apps/yard/commit/b809bebb5d94c91f22ea1e87c940129ea488209e))
* **compress:** add pure grouping/counts/filter lib for run board ([6bf3c86](https://github.com/initgrep-apps/yard/commit/6bf3c866a711d329ced1767234a695783688fac2))
* **compress:** animated run-board rows (RunRow + Lane) ([be39958](https://github.com/initgrep-apps/yard/commit/be39958057e0a950dc9bb546e73770c966199f49))
* **compression:** backend-driven cancelled batch state ([d4b5173](https://github.com/initgrep-apps/yard/commit/d4b51732c35ea9b293bbe25852170742d6b9d60d))
* **compression:** backend-driven cancelled batch state ([76a73f2](https://github.com/initgrep-apps/yard/commit/76a73f25f5a697906dafd8b4567c02947eac994a))
* **compression:** capture strategy metadata and cancelled-batch toast ([aee572e](https://github.com/initgrep-apps/yard/commit/aee572e31e8e574e6bbc8cda7fd4ca676fc2c356))
* **compression:** capture strategy metadata and cancelled-batch toast ([1aba88f](https://github.com/initgrep-apps/yard/commit/1aba88fa45fb0b1b3d58627f2be452a735b20cb7))
* **compress:** kanban board + run header (Task 6) ([6c82dcd](https://github.com/initgrep-apps/yard/commit/6c82dcdfad87a3645437076134300db24cbf2986))
* **compress:** kanban board with chip filtering ([e44dcb7](https://github.com/initgrep-apps/yard/commit/e44dcb7cc951e917137af7b3efc827b579c8dbc5))
* **compress:** label palette and rezip strategy strings ([3820eb7](https://github.com/initgrep-apps/yard/commit/3820eb7d9212a4322d5be630051eb6dbb3ceba2c))
* **compress:** label palette and rezip strategy strings ([c0f4582](https://github.com/initgrep-apps/yard/commit/c0f4582decac3e8fd79c787b9eef3e57099bed04))
* **compress:** lane component with animated rows ([28c09dc](https://github.com/initgrep-apps/yard/commit/28c09dce3e2a49e1b98da94fd347323618b4d39e))
* **compress:** provider run-state + CompressView two screens ([a33fb23](https://github.com/initgrep-apps/yard/commit/a33fb23d0acd6645795eea69d806aff9c1de59e9))
* **compress:** provider-owned run screen state ([49223a8](https://github.com/initgrep-apps/yard/commit/49223a894d11efd3ea315ebc9385155efb8b81a3))
* **compress:** run header with status pill and interactive chips ([0cfb3c8](https://github.com/initgrep-apps/yard/commit/0cfb3c845d28156ad0a59b9eeb9bf8ff3b78d49f))
* **compress:** run row variants with strategy display ([39dbda3](https://github.com/initgrep-apps/yard/commit/39dbda3bc4b860621e40c8e2ca5741c44638229b))
* **compress:** staged-files badge on sidebar ([941ca84](https://github.com/initgrep-apps/yard/commit/941ca8475bd02f165055ff19eedd5a3de71595fb))
* **compress:** staged-files badge on sidebar ([5e1dd6e](https://github.com/initgrep-apps/yard/commit/5e1dd6e96532b17c8fa6194b12f067549adf4661))
* **compress:** strategy label mapping for completed rows ([3c2b0d7](https://github.com/initgrep-apps/yard/commit/3c2b0d79324025f872d377474306682264907c31))
* **compress:** strategy label mapping for completed rows ([9e2b4e4](https://github.com/initgrep-apps/yard/commit/9e2b4e46dc548098e13933f44db1abc2655966c1))
* **compress:** two-screen compress flow with locked run monitor ([96649c5](https://github.com/initgrep-apps/yard/commit/96649c541522a186136a78d598bbc81baecaed4b))
* **compress:** two-state DropZone with format pills and drag-hover count ([6de1559](https://github.com/initgrep-apps/yard/commit/6de155998534e5c12c145b33b11a874adf430070))
* **compress:** two-state DropZone with format pills and drag-hover count ([d836a43](https://github.com/initgrep-apps/yard/commit/d836a438095aab545af85d394b2aab5661a0aabc))
* **history:** add 90-day retention trim and clear ([549bc64](https://github.com/initgrep-apps/yard/commit/549bc6468a0db138f450091af46edf50744da242))
* **history:** add 90-day retention trim and clear ([6d3e8ed](https://github.com/initgrep-apps/yard/commit/6d3e8edc6483e729b6d9a1c8dfc907a4307ddcf9))
* **history:** add filter dialog with chips and date presets ([#166](https://github.com/initgrep-apps/yard/issues/166)) ([7e8b4c4](https://github.com/initgrep-apps/yard/commit/7e8b4c4154d112ef41db9efc59ff0b384b7cf6b6))
* **history:** add frontend types, query hook and format helpers ([58fa4ab](https://github.com/initgrep-apps/yard/commit/58fa4abdd828a16089f2dc4be74069b1a7ecf98e))
* **history:** add frontend types, query hook and format helpers ([fe7f33d](https://github.com/initgrep-apps/yard/commit/fe7f33dce711c20e1b6047ce05ec59d27be246e9))
* **history:** add newest/oldest sort to the history query ([#159](https://github.com/initgrep-apps/yard/issues/159)) ([aefdfa9](https://github.com/initgrep-apps/yard/commit/aefdfa9f0a94fd26d71392a716230625d2e36e56))
* **history:** add scrollable sidebar batch list and filter trigger ([#167](https://github.com/initgrep-apps/yard/issues/167)) ([5d07b26](https://github.com/initgrep-apps/yard/commit/5d07b2691c8a95a97b5ed929efaec278e8c5b842))
* **history:** build filterable session list UI with pagination ([5e112d2](https://github.com/initgrep-apps/yard/commit/5e112d2a9e44780c58c5bcd200ed9df5f311153e))
* **history:** build filterable session list UI with pagination ([93f18ca](https://github.com/initgrep-apps/yard/commit/93f18cac96c98a2acae6ae011f7a91a3b4be0c64))
* **history:** core-side query with filters, summary and pagination ([5342d03](https://github.com/initgrep-apps/yard/commit/5342d036b57991b50d89ec77bd7552653bd50dde))
* **history:** focus the history view on the selected batch ([#168](https://github.com/initgrep-apps/yard/issues/168)) ([8963b7a](https://github.com/initgrep-apps/yard/commit/8963b7a622b8d0be694fc296b218bdf13d811e4d))
* **history:** history data layer ([#160](https://github.com/initgrep-apps/yard/issues/160)) ([42fb0fb](https://github.com/initgrep-apps/yard/commit/42fb0fbc15617590692b22fba09ac3ea4d8c9229))
* **history:** redesign batch dashboard and file table ([#188](https://github.com/initgrep-apps/yard/issues/188)) ([3071316](https://github.com/initgrep-apps/yard/commit/307131647c46d6fe985389dd20249a434e511e49))
* icon change in sidebar ([#213](https://github.com/initgrep-apps/yard/issues/213)) ([d695cc0](https://github.com/initgrep-apps/yard/commit/d695cc090e437fede3f6f94f6c474acf5e99cc39))
* **ipc:** add get_history, clear_history and open_folder commands ([3da5850](https://github.com/initgrep-apps/yard/commit/3da58504dc8beb67a5cca90a05f927ffaa7c95f6))
* **ipc:** add get_history, clear_history and open_folder commands ([62ec134](https://github.com/initgrep-apps/yard/commit/62ec13416d239897b4b62232d12027ed18b4e9e2))
* **queue:** emit cancelled flag on batch complete ([1cdb5d7](https://github.com/initgrep-apps/yard/commit/1cdb5d77eef58e1e40dd6317366dd9cb34637415))
* **queue:** emit cancelled flag on batch complete ([a557c44](https://github.com/initgrep-apps/yard/commit/a557c44fd7dddec5d273dad57f1078a9eec58359))
* **queue:** record session, file and end records to history ([fbe2055](https://github.com/initgrep-apps/yard/commit/fbe2055a43eb34a14a4a33c3a48a810ef47ff499))
* **queue:** record session, file and end records to history ([265b93a](https://github.com/initgrep-apps/yard/commit/265b93af79e6f0807850d4c54da0f54847fe2da7))
* **run:** run board on stock kanban-03 ([#194](https://github.com/initgrep-apps/yard/issues/194)) ([10bd955](https://github.com/initgrep-apps/yard/commit/10bd955de205bee60092caab720d11e1d5feb4cc))
* **settings:** add history retention control ([401c84f](https://github.com/initgrep-apps/yard/commit/401c84fa1dffce90b9254503cc85a625bc1baf35))
* **settings:** add history retention control ([b3bfcd7](https://github.com/initgrep-apps/yard/commit/b3bfcd7267cf2e01c0678742a320681ac8188309))
* **settings:** move settings into an auto-saving dialog ([#164](https://github.com/initgrep-apps/yard/issues/164)) ([8940a8e](https://github.com/initgrep-apps/yard/commit/8940a8e58f8af472c948c963a4a967c19c212e80))
* **shell:** add menus, about dialog, and a native macOS menu without About ([#161](https://github.com/initgrep-apps/yard/issues/161)) ([64d1a84](https://github.com/initgrep-apps/yard/commit/64d1a848f8d030d6e48027e8a68a8e613ba79ff4))
* **shell:** add plugin-os platform detection and window chrome config ([#154](https://github.com/initgrep-apps/yard/issues/154)) ([1f7194a](https://github.com/initgrep-apps/yard/commit/1f7194a798f9214cf81c2100829e5c72d2b2a515))
* **shell:** drag surface, window controls, and quit guard ([#158](https://github.com/initgrep-apps/yard/issues/158)) ([059604c](https://github.com/initgrep-apps/yard/commit/059604cda3d14133cc86def2b29911068fdcf895))
* **shell:** replace the app shell with native-style chrome ([#165](https://github.com/initgrep-apps/yard/issues/165)) ([6351e3e](https://github.com/initgrep-apps/yard/commit/6351e3ecad086fc8d65939c561a6a78d385d0c66))
* **shell:** theme system and toggle ([#157](https://github.com/initgrep-apps/yard/issues/157)) ([2f31d61](https://github.com/initgrep-apps/yard/commit/2f31d614beaca5477b0a6187844512617824908d))
* **sidecar:** resolve staged gs resources and pass them as -I flags to gs ([72937ea](https://github.com/initgrep-apps/yard/commit/72937ea5b9f6204729a2dae9c063ee5222e5794c))
* **ui-bugs:** global overflow hidden + sidebar inset min-w-0 ([6393d52](https://github.com/initgrep-apps/yard/commit/6393d52b86a25529b7df3d1a06eedd966a3cbc9f))
* **ui-bugs:** global overflow hidden + sidebar inset min-w-0 ([6f72449](https://github.com/initgrep-apps/yard/commit/6f7244933fe2056a113a9219867414537929c0fe))
* **ui:** apply shadcn preset b0 with neutral palette ([#211](https://github.com/initgrep-apps/yard/issues/211)) ([bb9792a](https://github.com/initgrep-apps/yard/commit/bb9792ade57f74a214de1dd3d13f99fd5795cdb8))
* **ui:** content-driven sidebar auto-collapse hook ([268b05a](https://github.com/initgrep-apps/yard/commit/268b05a6661a50a0f9ff6611f3ae180e1c98fcdd))
* **ui:** content-driven sidebar auto-collapse hook ([b9db79f](https://github.com/initgrep-apps/yard/commit/b9db79f27db91292c4399c417bb2ef15ea53cfad))
* **ui:** history view tone-matched with kanban ([aeac582](https://github.com/initgrep-apps/yard/commit/aeac582f7d3096fab06b95b0b7dfb02267115430))
* **ui:** history view tone-matched with kanban (Task 10) ([efe089c](https://github.com/initgrep-apps/yard/commit/efe089ccb5a5c674171d29fd21b26ba0c8981dcf))
* **ui:** kanban card fidelity — gradient bars, tinted bg, done accent ([06f1677](https://github.com/initgrep-apps/yard/commit/06f1677e7b2c81355433dd6d3843c7f1e4500608))
* **ui:** kanban card fidelity — gradient bars, tinted bg, done accent ([f11d551](https://github.com/initgrep-apps/yard/commit/f11d551ea15954e96d53747b492a66e8fcc26268))
* **ui:** rounded drop zone with grouped labeled format pills ([6886ae9](https://github.com/initgrep-apps/yard/commit/6886ae9ef3e20a837de0b8d4e6941a288ee67330))
* **ui:** rounded drop zone with grouped labeled format pills ([95ece43](https://github.com/initgrep-apps/yard/commit/95ece435f32fcad98017636eec160c0150eeb24e))
* **ui:** settings view tinted icons and badges ([00e4883](https://github.com/initgrep-apps/yard/commit/00e48836a36e92e99201ab245eaf0f5321c08632))
* **ui:** settings view tinted icons and badges ([20a2dab](https://github.com/initgrep-apps/yard/commit/20a2dabaf740c78017a9c7fa74cbc3967e6fb03b))
* **ui:** shadcn primitives and shared CategoryIcon ([#155](https://github.com/initgrep-apps/yard/issues/155)) ([763c363](https://github.com/initgrep-apps/yard/commit/763c363a75052c10643404fa62f3d86711edff3f))
* **ui:** shared category and status tone module ([2330b0d](https://github.com/initgrep-apps/yard/commit/2330b0ddfcff346b7b8e61347d63bccf39ba32fa))
* **ui:** shared category and status tone module ([9089d65](https://github.com/initgrep-apps/yard/commit/9089d654814503a8d7d08e5dae2127985a4d9fac))
* **ui:** sidebar redesign with icon containers and pill footer ([e0be13a](https://github.com/initgrep-apps/yard/commit/e0be13ac973fe776a73bd2b4b63c0b47a68d717f))
* **ui:** sidebar redesign with icon containers and pill footer ([dbcffa2](https://github.com/initgrep-apps/yard/commit/dbcffa27db3e112e51a3d373bbbeb6af5e68a556))
* **ui:** staging table category badges use shared tones ([73231c7](https://github.com/initgrep-apps/yard/commit/73231c70ddf36a8604e8511d4daf64e6a719581d))
* **ui:** staging table category badges use shared tones ([03ce03d](https://github.com/initgrep-apps/yard/commit/03ce03d7bf6e1d29545e38487abb35a79c9c97b5))
* **ui:** update compressing card active accent with primary ring and pulse ([31562a8](https://github.com/initgrep-apps/yard/commit/31562a87f38ef8acc5b77ce077ff702bf81f9c47))
* **ui:** update compressing card active accent with primary ring and pulse animation ([94b138c](https://github.com/initgrep-apps/yard/commit/94b138c0c9515912edc707ca7d132f460f4ca391))
* **ui:** wire auto-collapse into views, raise window min width ([55e2c10](https://github.com/initgrep-apps/yard/commit/55e2c10651beb60681b0f98932adb9027d154f7b))
* **ui:** wire auto-collapse into views, raise window min width ([dba3ce7](https://github.com/initgrep-apps/yard/commit/dba3ce78718b2b98b7581bc866f296766a1d8867))
* **updater:** add manual signed updates ([#236](https://github.com/initgrep-apps/yard/issues/236)) ([a87ec7b](https://github.com/initgrep-apps/yard/commit/a87ec7bf43fd01c5b74367896906a436bad06453))


### Bug Fixes

* **about:** constrain third-party notices width ([#171](https://github.com/initgrep-apps/yard/issues/171)) ([81dfa23](https://github.com/initgrep-apps/yard/commit/81dfa23596f2cf72e006bb76078f1ade22cf7670))
* **audit:** echo the invoked windows gs exe in both smokes and lock the flat static fallback ([d8f8304](https://github.com/initgrep-apps/yard/commit/d8f830499a93f6aa504783c801a49d4dc5257480))
* **audit:** resolve the smoke's gs binary through the node resolver, never bash -f probes ([d9c2a64](https://github.com/initgrep-apps/yard/commit/d9c2a6460a61a0bdce17ecf7016288e34d6c2310))
* **audit:** run the windows gs rom smoke from the engines dir exe rather than the dll-less flat stub ([83574c9](https://github.com/initgrep-apps/yard/commit/83574c956c9ebc99499204a855b1f29ad4e4ef4f))
* **audit:** run the windows gs smoke on the rom model ([160ada6](https://github.com/initgrep-apps/yard/commit/160ada67cf415b05d324159b51673487bf518a42))
* **binaries:** add the lowercase frameworks rpath the repo layout needs ([a9dcb49](https://github.com/initgrep-apps/yard/commit/a9dcb49fb33e89b9c542999f09a62775cbc7b7fe))
* **binaries:** address review — staging wipe, download hardening, strict manifest schema, audit shell test ([d4f8bf6](https://github.com/initgrep-apps/yard/commit/d4f8bf6a82acef1bbdc13cc042ca87873b562e54))
* **binaries:** arm the body trap repo-wide, stop gating brew rows on local hashes ([946f39d](https://github.com/initgrep-apps/yard/commit/946f39d31dcbf6bdbd709dbbf2684281055615b3))
* **binaries:** BINARIES_ALLOW_INACTIVE_BREW=1 covers any brew-list failure ([000d462](https://github.com/initgrep-apps/yard/commit/000d46251d8a3f4e23cc672b05bb4dbda480547f))
* **binaries:** close silent-success path in prep frameworks gate, fix two dead-end messages ([1574dee](https://github.com/initgrep-apps/yard/commit/1574dee664d137d9c8bea4b36c30942f3d694067))
* **binaries:** enable errtrace so the audit trap reaches function bodies ([6e5b9e2](https://github.com/initgrep-apps/yard/commit/6e5b9e29637d32e0e015faf8b8c80d2de444d4ed))
* **binaries:** fail closed on an empty sentinel list, pin the Rust sentinel contract ([f54a83b](https://github.com/initgrep-apps/yard/commit/f54a83b8d07f419b66668d391595d39a60e9215f))
* **binaries:** fail closed on audit enumeration and body aborts ([79f9349](https://github.com/initgrep-apps/yard/commit/79f9349d8ee32023b7ff73e0008d4adb23067671))
* **binaries:** gate every gs Resource subdir, ignore staging dirs ([9e1240d](https://github.com/initgrep-apps/yard/commit/9e1240de4d75409ad27e6dbe6d5ef64e925b52e4))
* **binaries:** gitignore and clear stale sidecar restore markers ([e8d334a](https://github.com/initgrep-apps/yard/commit/e8d334a8cea9642d57c383a6ad318cf7ad608fbb))
* **binaries:** guard the remaining subshells, single audit ERR handler ([ac99a48](https://github.com/initgrep-apps/yard/commit/ac99a486db470df8c01ea6d9f6b72a43891ba10f))
* **binaries:** honour audit --target, make fresh-clone failure actionable ([42324b5](https://github.com/initgrep-apps/yard/commit/42324b55a5ff850068c40ee48f89cb7165a27429))
* **binaries:** make the font-dir sentinel load-bearing, fix the dead exec check, pin the staged qpdf lib ([4ffd3b3](https://github.com/initgrep-apps/yard/commit/4ffd3b35cba24bf2a5f2ff76da52fa2313856de8))
* **binaries:** make the gs smoke, brew-drift and qpdf-lib gates load-bearing ([ed002ae](https://github.com/initgrep-apps/yard/commit/ed002ae02546e384f5baebff040a87744d276abe))
* **binaries:** make the verification script suite windows-portable ([6110804](https://github.com/initgrep-apps/yard/commit/6110804cf8e7e3aca804dc53bfe7d080aabe3b5f))
* **binaries:** mark windows ffmpeg/ffprobe as static flat externalBin engines ([20dce9a](https://github.com/initgrep-apps/yard/commit/20dce9a8e1b43a932c2cc68fee1e3a4ecca70c27))
* **binaries:** real download timeout, redirect scheme gate, silent-audit fix ([69ee26d](https://github.com/initgrep-apps/yard/commit/69ee26d41f981786c78c617e7908e6b0790049ed))
* **binaries:** remove incorrect engine candidates for Windows dev layout ([6f92012](https://github.com/initgrep-apps/yard/commit/6f92012935254f1c1abed335db97bbb5c0f66e73))
* **binaries:** report smoke aborts and clean its workdir ([120d3b5](https://github.com/initgrep-apps/yard/commit/120d3b5d3f8014dabfeaf4978332332e29c31c23))
* **binaries:** sentinels must not mask empty gs resource dirs, validate before rm -rf ([954a80a](https://github.com/initgrep-apps/yard/commit/954a80a24d8fb259b1acc1d22b540a946fda1390))
* **binaries:** skip engines staging for static windows manifest entries ([750a9a4](https://github.com/initgrep-apps/yard/commit/750a9a476b9960a0d1c90382738379908381af6f))
* **binaries:** skip the tauri.conf.json dylib cross-check on windows targets ([48f5806](https://github.com/initgrep-apps/yard/commit/48f580664377ac1b371e0038a0c36e57b49aa19b))
* **binaries:** treat a missing brew formula as inactive drift gate ([8fef767](https://github.com/initgrep-apps/yard/commit/8fef7678858d64931e810b6b0731c047bc85a8aa))
* **binaries:** treat windows ghostscript as a rom engine with no staged resources ([aa19bab](https://github.com/initgrep-apps/yard/commit/aa19bab9ef0dfeb084caca9c0f512238a388f113))
* **binaries:** use native Windows bsdtar instead of git-bash GNU tar for tar.zst extraction ([615e079](https://github.com/initgrep-apps/yard/commit/615e079fdb1ff2f7d7eda5c31cc29421636a2113))
* **build:** rebuild when app icons change ([#192](https://github.com/initgrep-apps/yard/issues/192)) ([cee1e7a](https://github.com/initgrep-apps/yard/commit/cee1e7a8198a2334b6bccf85fd3327542877c865))
* **bundle:** list brotli dylibs from CI freeze graph ([f0cecd7](https://github.com/initgrep-apps/yard/commit/f0cecd731af326bd78d55e4e9a44a3af8bfd8b6e))
* **bundle:** list brotli dylibs from CI freeze graph ([eab1068](https://github.com/initgrep-apps/yard/commit/eab1068990e2549c488c4381479614b10f80b874))
* **ci:** address PR review round 1 feedback ([8466cd9](https://github.com/initgrep-apps/yard/commit/8466cd95aecaaf8c578b98d59e886b6c5d3579f6))
* **ci:** address PR review round 1 for freeze/unfreeze scripts ([5869cee](https://github.com/initgrep-apps/yard/commit/5869cee67f5fe30c10ef03aef807979f6ddd319d))
* **ci:** address PR review round 2 feedback ([8c79be1](https://github.com/initgrep-apps/yard/commit/8c79be1711036597fe0536f62ad20efb66c963d1))
* **ci:** address PR review round 2 feedback ([53eb7df](https://github.com/initgrep-apps/yard/commit/53eb7df5c1c9f3a1b00ca7fdbd738632124d888f))
* **ci:** address PR review round 2 for freeze/unfreeze scripts ([3cd8625](https://github.com/initgrep-apps/yard/commit/3cd86257efa1432a7b09987c8c9cbdcb52dc978c))
* **ci:** address PR review round 3 for freeze/unfreeze scripts ([f037ccc](https://github.com/initgrep-apps/yard/commit/f037ccc283db3cbb7b9153d4a02af633cf754630))
* **ci:** address PR review round 4 feedback ([147c4d4](https://github.com/initgrep-apps/yard/commit/147c4d46a28bb88cbdd0aa49d06d975349aea23c))
* **ci:** classify parent-list failures fail-closed in sidecars preflight ([ac32124](https://github.com/initgrep-apps/yard/commit/ac32124713792c4ba961e4d11806ed39a4f3d11a))
* **ci:** derive the macOS frameworks list from the staged tree instead of a hand-curated conf ([3e80345](https://github.com/initgrep-apps/yard/commit/3e80345431d8850c7c7e448e95619784fccf5992))
* **ci:** derive windows freeze/unfreeze engine requirements from the manifest ([1528a93](https://github.com/initgrep-apps/yard/commit/1528a93469e903d3e83e40f99b1cbc2260f8bb40))
* **ci:** freeze windows bundles without gs-resources ([64ab6c5](https://github.com/initgrep-apps/yard/commit/64ab6c552b92f12da3330444f599dddcc4e957a9))
* **ci:** gate draft token, numeric release id, per-row asset check, drift allowance ([0411019](https://github.com/initgrep-apps/yard/commit/04110191d23ebcb5be2d0cd06cc9f95a935aa989))
* **ci:** gate unix-fixture tests for the windows-target cargo compile ([996ac90](https://github.com/initgrep-apps/yard/commit/996ac90f4b8a3a5734a46dc455796c8818fbeb1b))
* **ci:** give the dll-scan mktemp templates trailing X's for git-bash ([a5c8492](https://github.com/initgrep-apps/yard/commit/a5c849263a16710ea55fe163bd5cb0e49bc4d808))
* **ci:** harden snapshot packaging for all triples ([ec0e8b3](https://github.com/initgrep-apps/yard/commit/ec0e8b3b0576c83e8dc569677d27d974345af044))
* **ci:** install drift-gate formulas in freeze, allow inactive gate in release audit ([b9d299c](https://github.com/initgrep-apps/yard/commit/b9d299cdfdda39609bccbb1593792870d088fa1e))
* **ci:** keep release-version-check tests from writing the runner GITHUB_ENV ([8192a81](https://github.com/initgrep-apps/yard/commit/8192a81823454bb43db251a01b8cf91934a33e33))
* **ci:** make the verification suites runner-portable ([7bfbfa3](https://github.com/initgrep-apps/yard/commit/7bfbfa3e64b41a4f824f6e83885a8212f175d89b))
* **ci:** normalize runner homebrew-core tap; refresh pruned BtbN ffmpeg pins ([fb5f831](https://github.com/initgrep-apps/yard/commit/fb5f831bd86d86aa541527f1eaf806b8e4eaee3f))
* **ci:** normalize Windows backslash argv[0] in freeze/unfreeze sibling resolution ([129c918](https://github.com/initgrep-apps/yard/commit/129c918e91a42fdff5336be5b3b96843e39beece))
* **ci:** pass --force-local to tar on windows rows so drive-letter paths stop parsing as remote specs ([0b044ff](https://github.com/initgrep-apps/yard/commit/0b044ffcd623410c5d3087197ae5e00a885a87ff))
* **ci:** per-arch homebrew-core pins — Intel frozen at last sonoma-bottled engines ([84c58fb](https://github.com/initgrep-apps/yard/commit/84c58fb40d78e09e951bb7045517e98e3bb9a3fe))
* **ci:** pin Intel to the newest homebrew-core commit with a fully sonoma-bottled vips closure ([ce3610c](https://github.com/initgrep-apps/yard/commit/ce3610cb2d88ec5905b0c347aa95a23a6eee5007))
* **ci:** portability sweep — guard darwin-only test mechanics for the windows gate row ([cc750c1](https://github.com/initgrep-apps/yard/commit/cc750c128ef9c3447eedd89fb68510a38f7dad67))
* **ci:** pure-JS zstd fallback for .tar.zst extraction on windows rows ([6ee28cb](https://github.com/initgrep-apps/yard/commit/6ee28cbf2681940972a78709547e85ab0a376bf3))
* **ci:** raise testing-library waitFor cap to 10s ([1641436](https://github.com/initgrep-apps/yard/commit/1641436a76fb28b2b228102b52dfe9ad727906a5))
* **ci:** raise vitest testTimeout to 30s for constrained release runners ([60a981f](https://github.com/initgrep-apps/yard/commit/60a981fe22a137e7f53c511e37a449490cb93fe1))
* **ci:** re-add the macOS frameworks sync to the consumer ci/release rows ([25af198](https://github.com/initgrep-apps/yard/commit/25af198613c187989f221ad6650b59ab54a1ea7a))
* **ci:** refresh Homebrew index before freeze formula installs ([6146f7d](https://github.com/initgrep-apps/yard/commit/6146f7d7433373121cac89e996cac2bf8b177e7e))
* **ci:** resolve release tag from trigger env, pre-create draft via gate job ([22f4a35](https://github.com/initgrep-apps/yard/commit/22f4a3545de7103e2eade7a2d7eb546aa10ea9bf))
* **ci:** restore windows freezes into binaries only ([12e0cce](https://github.com/initgrep-apps/yard/commit/12e0cceb9c34642e8e840c66c848320cf1e8de58))
* **ci:** retry freeze finalize asset verify through indexing lag and reuse leftover drafts ([7e427a2](https://github.com/initgrep-apps/yard/commit/7e427a20322948fab568242d52e5290eb2e2dd98))
* **ci:** runner-portable gate residue — audit env parity + win32 lock-suite skips ([68dab2e](https://github.com/initgrep-apps/yard/commit/68dab2eca17b5f361547b27fa6967a194ae3388f))
* cleaup labels and about ([#186](https://github.com/initgrep-apps/yard/issues/186)) ([d597961](https://github.com/initgrep-apps/yard/commit/d597961b2018e8551166d1640fbbe8315518db8d))
* **compress:** address PR review round 1 feedback ([e1b976b](https://github.com/initgrep-apps/yard/commit/e1b976bcb2b823ec868665e7c6622d4757a783f5))
* **compress:** address PR review round 1 feedback ([15c0aa1](https://github.com/initgrep-apps/yard/commit/15c0aa1febb61c58016bcbee3696e0c7b10979f1))
* **compress:** address PR review round 1 feedback ([98c5558](https://github.com/initgrep-apps/yard/commit/98c5558d934919bfb7b4a88b41965fc6e0edc871))
* **compress:** address PR review round 1 feedback ([16ac2db](https://github.com/initgrep-apps/yard/commit/16ac2db7a3312d434ddb3cca3afcf9b0a5d711b9))
* **compress:** address PR review round 1 feedback ([a5e8cba](https://github.com/initgrep-apps/yard/commit/a5e8cbad284040889b56c55804f3aae48181d24d))
* **compress:** address PR review round 1 feedback ([efe7cb8](https://github.com/initgrep-apps/yard/commit/efe7cb803d3850af1ebe2a07e3ee0ad4d925f216))
* **compress:** address PR review round 2 feedback ([1caf82d](https://github.com/initgrep-apps/yard/commit/1caf82df8a7ec74570bea5f77672259b34ac684f))
* **compress:** address PR review round 3 feedback ([22cfe0f](https://github.com/initgrep-apps/yard/commit/22cfe0f949496a0b360bbd97872d9c6388b435f5))
* **compress:** bound the empty staging drop card ([#181](https://github.com/initgrep-apps/yard/issues/181)) ([601b985](https://github.com/initgrep-apps/yard/commit/601b985903860cb875391d6219a99ca9412cb99b))
* **compress:** close the persisted-batch-error mid-run preemption chain ([aa480a1](https://github.com/initgrep-apps/yard/commit/aa480a1d46ed7af9432c25d5af6d4ca93f34bced))
* **compression:** address PR review round 1 feedback ([d6c62f3](https://github.com/initgrep-apps/yard/commit/d6c62f36e8e0857bdf543706e154e5be357eff75))
* **compression:** address PR review round 1 feedback ([2493f68](https://github.com/initgrep-apps/yard/commit/2493f68adc5fd2729c7039c13bead37f017e4642))
* **compression:** address PR review round 1 feedback ([4596e4d](https://github.com/initgrep-apps/yard/commit/4596e4defc5055411d5dd0c426f660836da3a4ca))
* **compression:** address PR review round 2 feedback ([033f8dc](https://github.com/initgrep-apps/yard/commit/033f8dc23aec013963d82f63c84f190edfbf04b5))
* **compression:** clear list snapshot guard and test pins ([190f9ca](https://github.com/initgrep-apps/yard/commit/190f9ca7335af99339d8c813f227447b475b3dc9))
* **compression:** new batch clears staging list ([fea09ea](https://github.com/initgrep-apps/yard/commit/fea09ea4df3408e37dce52d01be9c4a0ce8e3dfc))
* **compression:** new batch clears staging list ([f33a64c](https://github.com/initgrep-apps/yard/commit/f33a64c80d3dd26d0a338fada96e9226c0018de4))
* **compression:** terminal watchdog for lost batch events ([8d0aef5](https://github.com/initgrep-apps/yard/commit/8d0aef53e1f8ac8838ab1580bfd5ca70ef0e456b))
* **compression:** terminal watchdog for lost batch events ([bb0cf41](https://github.com/initgrep-apps/yard/commit/bb0cf419adad0591d4ca14dc794c7e13827bdd20))
* **core:** exhaustive enum match pins, drop dead localStorage reset ([7b64403](https://github.com/initgrep-apps/yard/commit/7b6440381a762b473c561c6ab613c74e96d5b2a7))
* **core:** pin Rust-TS enum serde parity, add app-level ErrorBoundary ([aa8bd3c](https://github.com/initgrep-apps/yard/commit/aa8bd3c6fbb9e255ae50bf60bf81974068f1c408))
* **core:** pin Rust↔TS enum serde parity, add app-level ErrorBoundary ([7c8d8b7](https://github.com/initgrep-apps/yard/commit/7c8d8b7fbfb2f02990b0397423a97602ca6be94a))
* **dropzone:** pin secondary-slot invariance, arbitrary-key inertness and drop-reset contracts ([8c08863](https://github.com/initgrep-apps/yard/commit/8c088631754110c36ac31f879ef3cf0dbfe20033))
* **history:** cap page-sync corrections and guard invalid timestamps ([1e0e4ba](https://github.com/initgrep-apps/yard/commit/1e0e4ba05b6cd1496abbb263fce2171b4e03f124))
* **history:** clamp empty responses and warn on manage collision ([e311b2c](https://github.com/initgrep-apps/yard/commit/e311b2c5c23cbafe1e60e1b4ba43e5bb3381295d))
* **history:** clear tmp before main and harden trim durability ([2b86a6c](https://github.com/initgrep-apps/yard/commit/2b86a6c4715efd52635359a3df2d8286d8e393ae))
* **history:** gate page reset, log clear failures and degraded listeners ([7953709](https://github.com/initgrep-apps/yard/commit/7953709c259eb627b6c6428dca15a4bc74372ae7))
* **history:** guard dropped-count cast and reuse write helper ([10ae3ba](https://github.com/initgrep-apps/yard/commit/10ae3ba964547b80a95a4a457cdaac4beef7b945))
* **history:** harden page sync and log action failures ([9768b8f](https://github.com/initgrep-apps/yard/commit/9768b8ffd9d97b03eee691a751024334b2dc545d))
* **history:** keep table columns from overlapping at narrow widths ([#187](https://github.com/initgrep-apps/yard/issues/187)) ([425dd23](https://github.com/initgrep-apps/yard/commit/425dd23e3c21bc80fe424b43b23251797129623a))
* **history:** polish toolbar and retire working docs ([#190](https://github.com/initgrep-apps/yard/issues/190)) ([266f1c1](https://github.com/initgrep-apps/yard/commit/266f1c1be95802b06f9c0aeea7c3a19bca467bb8))
* **history:** preserve multi-select chips when removing one and keep pressed-state semantics ([c5cd2a7](https://github.com/initgrep-apps/yard/commit/c5cd2a72cbcc570bafa9b5d93539befeb1cc84f4))
* **history:** surface reveal and open-folder failures with toasts ([1193f7a](https://github.com/initgrep-apps/yard/commit/1193f7afdd00dcfa343d9a3f7e49031d06e37549))
* **history:** sync clamped page and polish zero/parentDir/keys ([ecae8fd](https://github.com/initgrep-apps/yard/commit/ecae8fdd3ebafb4b975d9921cffc634ec80e4c19))
* **history:** widen row menu and shorten reveal labels ([773f0a3](https://github.com/initgrep-apps/yard/commit/773f0a34879fff688faff9536c6fffcf28b7d039))
* **ipc:** genericize cancel_all and warn on queue-state collision ([43f45ed](https://github.com/initgrep-apps/yard/commit/43f45edd745101e109154992205b5cf30fe55654))
* **ipc:** warn on missing history store and clamp via MAX_PAGE_SIZE ([b9e651b](https://github.com/initgrep-apps/yard/commit/b9e651b2fef6d0ec6e3389a61a4b818a27b40954))
* **macos:** let the ad-hoc-signed helpers load their dylibs ([#143](https://github.com/initgrep-apps/yard/issues/143)) ([a092050](https://github.com/initgrep-apps/yard/commit/a092050b6cafc19bb97e7d725f1b4fb2752e1710))
* **queue:** log history:error emit failures and clear flags on preflight ([35472b8](https://github.com/initgrep-apps/yard/commit/35472b8c3052642168155539682039eaeb51149a))
* **queue:** once-per-batch missing-store warn with context ([bb672f5](https://github.com/initgrep-apps/yard/commit/bb672f5974f6addbb4052ec195ef197f2a61daf6))
* **queue:** record lane pending-cancelled files and mid-flight duration ([e37006e](https://github.com/initgrep-apps/yard/commit/e37006e6d0a8f47dc696f4c3062111d0ff264c46))
* **queue:** stabilize cancelled-flag test against worker race ([01e6134](https://github.com/initgrep-apps/yard/commit/01e6134fe883c84553c05fb430aa68bece96c275))
* **queue:** warn on missing store and unknown statuses ([e45f8b5](https://github.com/initgrep-apps/yard/commit/e45f8b50b830bf2b6a24ee39967bd13dab87dde3))
* **release:** reuse release-please draft releases ([#229](https://github.com/initgrep-apps/yard/issues/229)) ([0bbcc77](https://github.com/initgrep-apps/yard/commit/0bbcc77146f73d426e236cd7e31777d62b6089a4))
* **run:** axis-locked drag ghosts, in-container insets, lane-sized ghost ([#200](https://github.com/initgrep-apps/yard/issues/200)) ([4a8ea21](https://github.com/initgrep-apps/yard/commit/4a8ea21ca88c15995923c78479a447092fe839e8))
* **run:** bound board height chain and size column drag ghost to lane height ([#197](https://github.com/initgrep-apps/yard/issues/197)) ([5ba4bd4](https://github.com/initgrep-apps/yard/commit/5ba4bd43a4b17a07cd43ce7e17ad977ab7d51298))
* **run:** ordered keyboard targets and in-container lane insets ([#199](https://github.com/initgrep-apps/yard/issues/199)) ([5c7adb7](https://github.com/initgrep-apps/yard/commit/5c7adb7d99e92248bfa07c1c27274b6d3bce8b23))
* **run:** scoped keyboard drag coordinates and stock lane paddings ([#198](https://github.com/initgrep-apps/yard/issues/198)) ([3c3b987](https://github.com/initgrep-apps/yard/commit/3c3b98725c6b3d1b40637d846ee88007a18fdb8b))
* **scripts:** pin mac fixture arches; add windows-arm host branch ([2aa38fe](https://github.com/initgrep-apps/yard/commit/2aa38fe4cc8bc0e30d0251d9fdc6ca8db7194c86))
* **settings:** align history retention visible label with accessible name ([f26278d](https://github.com/initgrep-apps/yard/commit/f26278d8be9a002ccd42fdb6ba57ad1905362957))
* **settings:** plain-language labels and section copy ([#227](https://github.com/initgrep-apps/yard/issues/227)) ([087679a](https://github.com/initgrep-apps/yard/commit/087679a2ab1704dd12c7089dc6d496fdac4ac03e))
* **settings:** sidebar-style tabs and destination directory ([#228](https://github.com/initgrep-apps/yard/issues/228)) ([2a6d279](https://github.com/initgrep-apps/yard/commit/2a6d279aff52984700f6eb41071aaeb9edb96e43))
* **shell:** fill sidebar history rows and drop target badges ([#179](https://github.com/initgrep-apps/yard/issues/179)) ([2f8f7b3](https://github.com/initgrep-apps/yard/commit/2f8f7b3e50beb8060a05a95b75b5d3047c988d9d))
* **shell:** hide empty history chrome and use the app icon ([#172](https://github.com/initgrep-apps/yard/issues/172)) ([e2ef777](https://github.com/initgrep-apps/yard/commit/e2ef777f941f0037dd6461ea5ea88a7035828a15))
* **shell:** remove windows brand chip and align alpha versioning ([#184](https://github.com/initgrep-apps/yard/issues/184)) ([abcae86](https://github.com/initgrep-apps/yard/commit/abcae863b7b5841c89e709e6fad5d821a54df2d2))
* **shell:** sidebar chrome and item rows ([#180](https://github.com/initgrep-apps/yard/issues/180)) ([8be41ab](https://github.com/initgrep-apps/yard/commit/8be41abf53929dd22f95faea10aadd8e672fb527))
* **shell:** use square glyph for window maximize control ([7062b79](https://github.com/initgrep-apps/yard/commit/7062b7959f08a37aca8a6874d3be83485225208f))
* **src-tauri:** embed windows app manifest into test executables ([783d202](https://github.com/initgrep-apps/yard/commit/783d202ceb1429b1e52c0d8a74475464dcaa3927))
* **src-tauri:** gate shell-spawning opener tests off windows runners ([9da8fd8](https://github.com/initgrep-apps/yard/commit/9da8fd8f33999c9cc5fe1ca648f1cee6b1eaed08))
* **src-tauri:** keep cargo tests runner-portable on windows fixtures ([554287e](https://github.com/initgrep-apps/yard/commit/554287e8d8b7b505274d34d8baad6dac9f87dc88))
* **tests:** stub the fixture rustc host triple in bundle-pdf staging-gate tests ([3766201](https://github.com/initgrep-apps/yard/commit/37662010b13bb5e09787de571ad7ebebdd83612e))
* **tests:** stub the fixture rustc host triple in bundle-pdf staging-gate tests ([50b3ce5](https://github.com/initgrep-apps/yard/commit/50b3ce53b0b81ef0dcfcf79ab22c87c0c802c57d))
* **tokens:** harden tone test pins per round 2 review ([8abd7d3](https://github.com/initgrep-apps/yard/commit/8abd7d33fd0831ad79e5bddcdedc413ef5e7a402))
* **ui:** address PR review round 1 feedback ([9a9cfd0](https://github.com/initgrep-apps/yard/commit/9a9cfd035062130018c071bb158b0a12aee1e39a))
* **ui:** address PR review round 1 feedback ([ec1dee5](https://github.com/initgrep-apps/yard/commit/ec1dee5a8d9c822593d0f7de8d32905240717ef1))
* **ui:** address PR review round 2 feedback ([cfc5297](https://github.com/initgrep-apps/yard/commit/cfc5297819d192799cb965f237c32f4178ca53a9))
* **ui:** auto-collapse observer stability and manual-toggle soundness ([fe878ae](https://github.com/initgrep-apps/yard/commit/fe878ae09e7ebfb81f8c38fd5082a18f875007b2))
* **ui:** compact drop bar two-row layout, no flicker ([d72562e](https://github.com/initgrep-apps/yard/commit/d72562ea7fdc9d61298bc01c0b52bc08fef9061a))
* **ui:** dropzone grouping invariants and placement pins ([635a40b](https://github.com/initgrep-apps/yard/commit/635a40b45e8717e972adb915bd9a7de0e2f30867))
* **ui:** history chip dark-mode tint and accent edge pins ([6ec5024](https://github.com/initgrep-apps/yard/commit/6ec50240cc22665a7d177b59e7affbfcdd0ff48d))
* **ui:** history data table, kanban scroll, shell fixes ([#183](https://github.com/initgrep-apps/yard/issues/183)) ([b0d96d1](https://github.com/initgrep-apps/yard/commit/b0d96d1a5972d0b821c3782268bd0aa8178ccb20))
* **ui:** inline verbatim accent shadow constant for tailwind extraction ([f3819b5](https://github.com/initgrep-apps/yard/commit/f3819b50c1ed333d768b085abd2ac1f7fd51f562))
* **ui:** Kanban responsive + scroll fixes ([19d75ba](https://github.com/initgrep-apps/yard/commit/19d75bab46df6a029a52355719c218024c39d386))
* **ui:** Kanban responsive + scroll fixes (Task 3) ([fefe5d9](https://github.com/initgrep-apps/yard/commit/fefe5d9b858f9e7ad81ab72218e027432a116bbc))
* **ui:** left-align sidebar footer ([995783f](https://github.com/initgrep-apps/yard/commit/995783faadc3573eada32e9a1d825a4619eaed9c))
* **ui:** left-align sidebar footer version and GPLv3 ([2c705a6](https://github.com/initgrep-apps/yard/commit/2c705a69e0a48df2f88a8bba1fa59c92f28f9e3d))
* **ui:** plain-language settings, editable manual ceiling, responsive board ([#226](https://github.com/initgrep-apps/yard/issues/226)) ([3a26ad5](https://github.com/initgrep-apps/yard/commit/3a26ad5f12513ada496522e3a16b76f4360de5e3))
* **ui:** remove kanban right-edge overlay, add scroll-end padding ([3f6ea4b](https://github.com/initgrep-apps/yard/commit/3f6ea4bfc4919a71b8b42ed70437da55ca2f018a))
* **ui:** remove kanban right-edge overlay, add scroll-end padding ([4281996](https://github.com/initgrep-apps/yard/commit/428199609fd4bca76d1407a632366d0b34a2e2a5))
* **ui:** stop kanban auto-collapse flicker loop, calm active card ([4c833e9](https://github.com/initgrep-apps/yard/commit/4c833e9158bff7e857a519837642ac23d08fcb14))
* **ui:** widen kanban scroll-end padding so last lane pill is fully visible at max scroll ([d084647](https://github.com/initgrep-apps/yard/commit/d0846472fdd0966183788e028521c801020a8465))
* **updater:** read version from package in About test ([#239](https://github.com/initgrep-apps/yard/issues/239)) ([30a38a9](https://github.com/initgrep-apps/yard/commit/30a38a94d907f90692e178b70f188f5af753cf06))
* **windows:** let the app find ffmpeg/ffprobe beside the exe ([#145](https://github.com/initgrep-apps/yard/issues/145)) ([0756bd9](https://github.com/initgrep-apps/yard/commit/0756bd9ef04b693ebb66e315528fcfa6cdb6a59c))
* **windows:** ship the engines into resources/engines/ ([#144](https://github.com/initgrep-apps/yard/issues/144)) ([ec65943](https://github.com/initgrep-apps/yard/commit/ec659431d0f2dff47e12a77b062cb29afb143bda))
* **windows:** stop console windows flashing during compression ([#150](https://github.com/initgrep-apps/yard/issues/150)) ([2db07ad](https://github.com/initgrep-apps/yard/commit/2db07adc7145988f27dd079ccd4737b1b29e594e))

## [1.2.0](https://github.com/initgrep-apps/yard/compare/v1.1.0...v1.2.0) (2026-11-08)


### Features

* **updater:** add manual signed updates ([#236](https://github.com/initgrep-apps/yard/issues/236)) ([a87ec7b](https://github.com/initgrep-apps/yard/commit/a87ec7bf43fd01c5b74367896906a436bad06453))


### Bug Fixes

* **release:** reuse release-please draft releases ([#229](https://github.com/initgrep-apps/yard/issues/229)) ([0bbcc77](https://github.com/initgrep-apps/yard/commit/0bbcc77146f73d426e236cd7e31777d62b6089a4))
* **updater:** read version from package in About test ([#239](https://github.com/initgrep-apps/yard/issues/239)) ([30a38a9](https://github.com/initgrep-apps/yard/commit/30a38a94d907f90692e178b70f188f5af753cf06))

## [1.1.0](https://github.com/initgrep-apps/yard/compare/v1.0.0...v1.1.0) (2026-11-07)


### Features

* bounded memory, consent-gated fidelity, and backend remediation ([#224](https://github.com/initgrep-apps/yard/issues/224)) ([602c283](https://github.com/initgrep-apps/yard/commit/602c2831176f2df78d3f344ff11cff905f4618f1))


### Bug Fixes

* **settings:** plain-language labels and section copy ([#227](https://github.com/initgrep-apps/yard/issues/227)) ([087679a](https://github.com/initgrep-apps/yard/commit/087679a2ab1704dd12c7089dc6d496fdac4ac03e))
* **settings:** sidebar-style tabs and destination directory ([#228](https://github.com/initgrep-apps/yard/issues/228)) ([2a6d279](https://github.com/initgrep-apps/yard/commit/2a6d279aff52984700f6eb41071aaeb9edb96e43))
* **ui:** plain-language settings, editable manual ceiling, responsive board ([#226](https://github.com/initgrep-apps/yard/issues/226)) ([3a26ad5](https://github.com/initgrep-apps/yard/commit/3a26ad5f12513ada496522e3a16b76f4360de5e3))

## 1.0.0 (2026-11-04)

First stable release. Yard compresses images, video, PDF, and Office documents
offline, to per-category size limits, without uploading files.

### Highlights

- **Category targets** — images, video, PDF, and documents each get a size limit;
  defaults are editable in Settings.
- **Bundled engines** — libvips, FFmpeg, qpdf, and Ghostscript ship with the app.
  No downloads, accounts, or network access at runtime.
- **Originals are never modified** — compressed copies go to a separate output
  folder; metadata is stripped and HEIC/HEIF becomes JPEG, MOV becomes MP4.
- **Batch workflow** — parallel compression with per-file progress, per-file
  cancel, and a history audit trail with 90-day retention by default.
- **Native installers** — macOS arm64/Intel DMG and Windows x64/arm64
  NSIS installers, built and gated on native CI runners.
- **Offline by design** — no telemetry, no auto-update, no cloud processing.

### Platforms

- macOS 11+ (Apple Silicon and Intel)
- Windows 10+ (x64 and arm64)

### Known limitations

- macOS builds are ad-hoc signed and not notarized; first launch needs the
  documented quarantine removal.
- Windows builds are unsigned, so SmartScreen warns on first launch.
- Linux builds from source but is not packaged.

## 0.1.0-alpha.1 (2026-10-17)

First development release, built and verified by the new CI/CD pipelines.

### Features

- 2x2 native installer matrix: macOS (arm + intel) and Windows (x64 + arm), each row unfreezing its
  sidecar engines from the `sidecars-20261017` snapshot release and gating on the full build + test suite
  before tauri-action uploads to the draft ([release run 39065895512]: 4/4 green, 10 draft assets).
- Snapshot-once engine pipeline: `freeze.yml` pins homebrew-core per arch, stages and audits all four
  triples on GitHub-hosted runners, and publishes the exact-8-asset `sidecars-*` release that CI and
  the release rows consume; the local machine stays development-only.

### Fixes

- Release version check accepts semver prerelease tags beyond `-rcN` (alpha/beta).
- Intel mac runner test portability: mac fixtures are arch-explicit, bundle scripts are triple-safe,
  and the rustc-derived triple in `bundle-pdf` staging-gate tests is stubbed to the fixture triple.
