// @vitest-environment node
import { describe, expect, it } from "vitest";

import { existsSync, readdirSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const HERE = dirname(fileURLToPath(import.meta.url));
const WORKFLOWS_DIR = join(HERE, "..", "..", ".github", "workflows");

const EXPECTED_WORKFLOWS = [
  { file: "freeze.yml", status: "current" },
  { file: "release.yml", status: "current" },
  { file: "ci.yml", status: "current" },
  { file: "release-please.yml", status: "current" },
];

const FREEZE_MATRIX_ROWS = [
  { triple: "aarch64-apple-darwin", os: "macos-15", platform: "mac" },
  { triple: "x86_64-apple-darwin", os: "macos-15-intel", platform: "mac" },
  { triple: "x86_64-pc-windows-msvc", os: "windows-2025", platform: "windows" },
  { triple: "aarch64-pc-windows-msvc", os: "windows-11-arm", platform: "windows" },
];

const RELEASE_MATRIX_ROWS = [
  { triple: "aarch64-apple-darwin", os: "macos-15" },
  { triple: "x86_64-apple-darwin", os: "macos-15-intel" },
  { triple: "x86_64-pc-windows-msvc", os: "windows-2025" },
  { triple: "aarch64-pc-windows-msvc", os: "windows-11-arm" },
];

const GATE_COMMAND = "yarn build && yarn test && cd src-tauri && cargo test";

const GATE_MATRIX_ROWS = [
  { triple: "aarch64-apple-darwin", os: "macos-15" },
  { triple: "x86_64-pc-windows-msvc", os: "windows-2025" },
];

const APP_ASSET_PATTERNS = [
  { triple: "aarch64-apple-darwin", pattern: "aarch64\\." },
  { triple: "x86_64-apple-darwin", pattern: "x64\\." },
  { triple: "x86_64-pc-windows-msvc", pattern: "x64[-_]" },
  { triple: "aarch64-pc-windows-msvc", pattern: "arm64" },
];

const VALID_TRIGGERS = new Set([
  "workflow_dispatch",
  "push",
  "pull_request",
  "pull_request_target",
  "schedule",
  "release",
  "workflow_call",
]);

function isWorkflowFile(file) {
  return file.endsWith(".yml") || file.endsWith(".yaml");
}

function state() {
  const existing = existsSync(WORKFLOWS_DIR) ? readdirSync(WORKFLOWS_DIR) : [];
  const files = existing.filter(isWorkflowFile);
  return { existing, files };
}

async function loadWorkflow(file) {
  const text = await readFile(join(WORKFLOWS_DIR, file), "utf8");
  const doc = parse(text);
  return { text, doc };
}

function jobsOf(doc) {
  if (!doc || typeof doc !== "object") return {};
  return doc.jobs ?? {};
}

function stepsOf(job) {
  return Array.isArray(job?.steps) ? job.steps : [];
}

function stepUses(step) {
  return typeof step?.uses === "string" ? step.uses : "";
}

function stepRuns(step) {
  return typeof step?.run === "string" ? step.run : "";
}

function runIndexesByText(job, needle) {
  return stepsOf(job)
    .map((step, i) => (stepRuns(step).includes(needle) ? i : -1))
    .filter((i) => i >= 0);
}

describe("workflows validation (all files)", () => {
  it("every .github/workflows/*.{yml,yaml} parses and declares valid on: triggers", async () => {
    const { files } = state();
    expect(files.length, "at least one workflow file exists").toBeGreaterThan(0);
    for (const file of files) {
      const { doc } = await loadWorkflow(file);
      expect(doc, `${file} must parse to an object`).toBeTypeOf("object");
      expect("on" in doc, `${file} must declare an on: trigger section`).toBe(true);
      const triggers = !doc.on || typeof doc.on !== "object" ? {} : doc.on;
      for (const key of Object.keys(triggers)) {
        expect(VALID_TRIGGERS.has(key), `${file}: unknown trigger '${key}'`).toBe(true);
      }
    }
  });

  it("no workflow file anywhere contains a -latest runner label (Review Focus #5)", async () => {
    const { files } = state();
    for (const file of files) {
      const { text } = await loadWorkflow(file);
      expect(text.includes("-latest"), `${file}: found a -latest label`).toBe(false);
    }
  });

  it("every third-party action is pinned to a full commit SHA", async () => {
    const { files } = state();
    for (const file of files) {
      const { doc } = await loadWorkflow(file);
      for (const [jobName, job] of Object.entries(jobsOf(doc))) {
        for (const step of stepsOf(job)) {
          const uses = stepUses(step);
          if (!uses) continue;
          expect(
            /^[\w.-]+\/[\w.-]+@[0-9a-f]{40}$/.test(uses),
            `${file} job ${jobName}: '${uses}' must pin owner/repo@<full 40-char SHA>`,
          ).toBe(true);
        }
      }
    }
  });

  it("self-check: every workflow file is listed and every current EXPECTED_WORKFLOWS entry exists on disk", () => {
    const { files } = state();
    const known = new Set(EXPECTED_WORKFLOWS.map((wf) => wf.file));
    for (const file of files) {
      expect(known.has(file), `${file} not listed in EXPECTED_WORKFLOWS`).toBe(true);
    }
    for (const wf of EXPECTED_WORKFLOWS) {
      if (wf.status !== "current") continue;
      expect(files.includes(wf.file), `${wf.file} marked current but missing from .github/workflows`).toBe(true);
    }
    expect(EXPECTED_WORKFLOWS.find((wf) => wf.file === "freeze.yml")?.status).toBe("current");
  });

  it("no consumer workflow stages engines via scripts/binaries/all.mjs (Ruling: only freeze.yml stages)", async () => {
    const { existing } = state();
    for (const file of existing) {
      if (file === "freeze.yml") continue;
      const { text } = await loadWorkflow(file);
      expect(
        text.includes("scripts/binaries/all.mjs"),
        `${file}: manifest staging must never run in consumers — frozen bundles are the only engine source`,
      ).toBe(false);
    }
  });

  it("manifest staging never runs where brew could run (Ruling: brew only inside freeze.yml mac rows)", async () => {
    for (const file of ["release.yml", "ci.yml"]) {
      const { text } = await loadWorkflow(file);
      expect(text.includes("brew install"), `${file}: brew installs belong to freeze.yml only`).toBe(false);
      expect(text.includes("brew update"), `${file}: brew updates belong to freeze.yml only`).toBe(false);
    }
  });
});

describe("release-please.yml changelog automation", () => {
  const FILENAME = "release-please.yml";
  const RELEASE_PLEASE_SHA = "45996ed1f6d02564a971a2fa1b5860e934307cf7";

  it("is listed current and exists on disk", () => {
    expect(EXPECTED_WORKFLOWS.find((wf) => wf.file === FILENAME)?.status).toBe("current");
    expect(existsSync(join(WORKFLOWS_DIR, FILENAME)), `${FILENAME} must exist`).toBe(true);
  });

  it("runs on pushes to main and manual dispatch only (release PRs must not re-trigger it)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(Object.keys(doc.on ?? {})).toEqual(["push", "workflow_dispatch"]);
    expect(doc.on.push.branches).toEqual(["main"]);
    expect(doc.on.push.tags).toBeUndefined();
  });

  it("scopes permissions: read default, the release-please job writes contents + pull-requests", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.permissions).toEqual({ contents: "read" });
    expect(jobsOf(doc)["release-please"].permissions).toEqual({
      contents: "write",
      "pull-requests": "write",
    });
  });

  it("mints a GitHub App token when configured, then runs the SHA-pinned release-please-action against the committed config + manifest", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const job = jobsOf(doc)["release-please"];
    expect(job["runs-on"]).toBe("ubuntu-24.04");
    expect(job["timeout-minutes"]).toBe(15);
    expect(job.env?.RELEASE_PLEASE_APP_ID).toBe("${{ secrets.RELEASE_PLEASE_APP_ID }}");
    expect(job.env?.RELEASE_PLEASE_APP_PRIVATE_KEY).toBe(
      "${{ secrets.RELEASE_PLEASE_APP_PRIVATE_KEY }}",
    );
    const steps = stepsOf(job);
    expect(steps).toHaveLength(2);
    expect(steps[0].id).toBe("app-token");
    expect(steps[0].if).toBe(
      "${{ env.RELEASE_PLEASE_APP_ID != '' && env.RELEASE_PLEASE_APP_PRIVATE_KEY != '' }}",
    );
    expect(stepUses(steps[0])).toMatch(/^actions\/create-github-app-token@[0-9a-f]{40}$/);
    expect(steps[1].uses).toBe(
      `googleapis/release-please-action@${RELEASE_PLEASE_SHA}`,
    );
    expect(steps[1].with.token).toBe(
      "${{ steps.app-token.outputs.token || secrets.RELEASE_PLEASE_TOKEN || secrets.GITHUB_TOKEN }}",
    );
    expect(steps[1].with["config-file"]).toBe("release-please-config.json");
    expect(steps[1].with["manifest-file"]).toBe(".release-please-manifest.json");
  });

  it("config keeps tags v-prefixed (release.yml trigger intact), targets stable versions, drafts the release PR, and syncs the app version files", async () => {
    const config = JSON.parse(await readFile(join(HERE, "..", "..", "release-please-config.json"), "utf8"));
    const pkg = config.packages["."];
    expect(pkg["release-type"]).toBe("node");
    expect(pkg["include-component-in-tag"]).toBe(false);
    expect(pkg.prerelease).toBeUndefined();
    expect(pkg["draft-pull-request"]).toBe(true);
    expect(pkg["extra-files"]).toEqual([
      {
        type: "json",
        path: "src-tauri/tauri.conf.json",
        jsonpath: "$.version",
      },
      {
        type: "toml",
        path: "src-tauri/Cargo.toml",
        jsonpath: "$.package.version",
      },
    ]);
  });

  it("manifest is seeded at the current package version so no history predating the baseline enters the changelog", async () => {
    const manifest = JSON.parse(
      await readFile(join(HERE, "..", "..", ".release-please-manifest.json"), "utf8"),
    );
    const pkg = JSON.parse(await readFile(join(HERE, "..", "..", "package.json"), "utf8"));
    expect(manifest["."]).toBe(pkg.version);
  });

  it("CHANGELOG.md is seeded with the hand-written stable baseline, not a historic commit dump", async () => {
    const changelog = await readFile(join(HERE, "..", "..", "CHANGELOG.md"), "utf8");
    expect(changelog).toMatch(/## 1\.0\.0 \(2026-11-04\)/);
    expect(changelog).not.toMatch(/### Bug Fixes[\s\S]*#1[12][0-9]\b/);
  });
});

describe("freeze.yml freeze workflow", () => {
  const FILENAME = "freeze.yml";

  it("has workflow_dispatch as its only trigger", async () => {
    expect(existsSync(join(WORKFLOWS_DIR, FILENAME)), `${FILENAME} must exist`).toBe(true);
    const { doc } = await loadWorkflow(FILENAME);
    expect(Object.keys(doc.on ?? {})).toEqual(["workflow_dispatch"]);
  });

  it("scopes permissions: read default, gate reads, freeze reads, finalize writes", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.permissions).toEqual({ contents: "read" });
    expect(jobsOf(doc).gate.permissions).toEqual({ contents: "read" });
    expect(jobsOf(doc).freeze.permissions).toEqual({ contents: "read" });
    expect(jobsOf(doc).finalize.permissions).toEqual({ contents: "write" });
  });

  it("queues concurrent dispatches (workflow-level concurrency, cancel-in-progress: false)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.concurrency.group).toBe("freeze-${{ github.repository }}");
    expect(doc.concurrency["cancel-in-progress"]).toBe(false);
  });

  it("freeze job matrix includes exactly the four triple rows across both platforms", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    expect(freeze).toBeDefined();
    expect(freeze.strategy["fail-fast"], "one row's timeout must not cancel the other rows — each row carries its own evidence").toBe(false);
    expect(freeze.strategy.matrix.include).toEqual(FREEZE_MATRIX_ROWS);
  });

  it("freeze rows are bounded by a 40-minute timeout so a runner hang dies loudly, not after six hours", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(jobsOf(doc).freeze["timeout-minutes"]).toBe(40);
  });

  it("runs every job under an explicit timeout (invariant: no unbounded jobs)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      expect(
        typeof job["timeout-minutes"] === "number" && job["timeout-minutes"] > 0,
        `job ${jobName} must carry a positive timeout-minutes`,
      ).toBe(true);
    }
  });

  it("mac rows pin homebrew-core per arch to the committed shas instead of blindly installing off a stale runner index", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    const pin = stepsOf(freeze).find((s) => stepRuns(s).includes("scripts/ci/homebrew-core-pins.json"));
    expect(pin, "a homebrew-core pin step must resolve the committed per-arch pin file").toBeDefined();
    expect(pin["if"]).toBe("matrix.platform == 'mac'");
    expect(
      pin.env?.TRIPLE,
      "the pin must be selected by the row triple passed as env — never a workflow expression inside the run string",
    ).toBe("${{ matrix.triple }}");
    const run = pin.run;
    expect(run).toContain("set -euo pipefail");
    expect(run, "the pin must be resolved from the pins file by the row's triple env").toContain(
      "process.env.TRIPLE",
    );
    expect(run).toContain('if ! core="$(brew --repository homebrew/core 2>&1)"; then');
    expect(run).toContain('if [ ! -d "$core/.git" ]');
    expect(run, "a missing tap must be cloned at the pinned commit, not silently skipped").toContain(
      "git clone --filter=blob:none https://github.com/Homebrew/homebrew-core \"$core\"",
    );
    expect(run).toContain('git -C "$core" fetch --depth 1 origin "$pin"');
    expect(run).toContain('git -C "$core" checkout --detach "$pin"');
    expect(
      run,
      "the tap must be normalized before fetch/checkout — the runner image ships local modifications to the homebrew-core tap that abort the pinned checkout",
    ).toContain('git -C "$core" reset --hard HEAD 2>/dev/null || true');
    expect(
      run,
      "untracked tap files must be pruned too — leftover state blocks the pinned checkout",
    ).toContain('git -C "$core" clean -ffd 2>/dev/null || true');
    expect(
      run.indexOf('git -C "$core" reset --hard HEAD 2>/dev/null || true'),
      "tap normalization must run before the fetch so the pinned checkout cannot be blocked by tap dirt",
    ).toBeGreaterThan(-1);
    expect(run.indexOf('git -C "$core" reset --hard HEAD 2>/dev/null || true')).toBeLessThan(
      run.indexOf('git -C "$core" fetch --depth 1 origin "$pin"'),
    );
    expect(
      run,
      "the raw pin line must be validated as one 40-char hex sha before it touches git",
    ).toContain("grep -Eq '^[0-9a-f]{40}$'");
    expect(pin.shell).toBe("bash");
  });

  it("a missing or malformed homebrew-core pin fails with an actionable ::error naming the pin file — never a silent grep death under set -e", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const run = stepsOf(jobsOf(doc).freeze).find((s) => stepRuns(s).includes("scripts/ci/homebrew-core-pins.json")).run;

    expect(
      run,
      "the pin resolution must be guarded so a missing key is classified instead of aborted silently",
    ).toContain('if ! pin="$(node -e');
    expect(run).toContain(
      "::error::homebrew-core pin for $TRIPLE in scripts/ci/homebrew-core-pins.json is missing or unreadable — add a 40-char lowercase sha for $TRIPLE, then re-run",
    );
    expect(
      run,
      "the malformed-sha check must stay inline so set -e cannot swallow the classification",
    ).toContain("if ! printf '%s' \"$pin\" | grep -Eq '^[0-9a-f]{40}$'");
    expect(run).toContain(
      "::error::homebrew-core pin for $TRIPLE in scripts/ci/homebrew-core-pins.json is not a 40-char lowercase sha (got: $pin) — fix the pin file, then re-run",
    );
  });

  it("the homebrew-core pin git operations are wrapped with remediation messages (working-dir recovery vs network vs pruned pin)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const run = stepsOf(jobsOf(doc).freeze).find((s) => stepRuns(s).includes("scripts/ci/homebrew-core-pins.json")).run;

    expect(
      run,
      "tap dir creation must be classified, not swallowed by set -e",
    ).toContain("::error::cannot create the homebrew-core tap dir $core —");
    expect(
      run,
      "the clone must be guarded so network failures name the recovery",
    ).toContain("if ! git clone --filter=blob:none https://github.com/Homebrew/homebrew-core \"$core\"; then");
    expect(run).toContain("::error::homebrew-core tap clone into $core failed —");
    expect(
      run,
      "the fetch must be guarded and distinguish a network flake from a pruned pin",
    ).toContain('if ! git -C "$core" fetch --depth 1 origin "$pin"; then');
    expect(run).toContain("::error::git fetch of the homebrew-core pin $pin for $TRIPLE failed —");
    expect(run).toContain("network errors (flake, proxy or DNS): re-run the job");
    expect(run).toContain("'unknown revision' or 'object not found' errors: the pin no longer exists in homebrew-core");
    expect(run).toContain(
      "re-pin to a sha that still carries the pinned bottles in scripts/ci/homebrew-core-pins.json",
    );
    expect(
      run,
      "the checkout must be guarded so a corrupt tap points at the recovery command",
    ).toContain('if ! git -C "$core" checkout --detach "$pin"; then');
    expect(run).toContain("::error::git checkout of the homebrew-core pin $pin for $TRIPLE failed —");
    expect(run).toContain("remove the tap checkout $core and re-run the job to restore it");
  });

  it("the homebrew-core tap location query is guarded — a missing or broken brew must be classified with remediation, not die on line 1 of the pin step", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const run = stepsOf(jobsOf(doc).freeze).find((s) => stepRuns(s).includes("scripts/ci/homebrew-core-pins.json")).run;

    expect(
      run,
      "brew --repository must be guarded so a missing/broken brew prints actionable remediation",
    ).toContain('if ! core="$(brew --repository homebrew/core 2>&1)"; then');
    expect(
      run,
      "the ::error must name the failing command and point at a missing/broken Homebrew prefix",
    ).toContain("::error::brew --repository homebrew-core failed");
    expect(run).toContain("Homebrew is missing or its prefix is unreadable");
    expect(
      run,
      "the remediation must name the restore-the-prefix path instead of restaging suggestions",
    ).toContain("restore the Homebrew prefix");
  });

  it("the per-arch pin file records one distinct hex40 sha per mac triple — Intel is frozen at the last homebrew-core commit whose full vips+gs closure is sonoma-bottled for x86_64 (per-formula bottle deaths start 2026-08-28: openssl@3 3.6.4; the death completes 2026-09-10/11)", async () => {
    const pinsPath = new URL("../../scripts/ci/homebrew-core-pins.json", import.meta.url);
    expect(existsSync(pinsPath), "the per-arch pin file must exist").toBe(true);
    expect(
      existsSync(new URL("../../scripts/ci/homebrew-core.sha", import.meta.url)),
      "the single-sha pin file must be retired — per-arch pins replaced it",
    ).toBe(false);
    const pins = JSON.parse(await readFile(pinsPath, "utf8"));
    expect(Object.keys(pins).sort()).toEqual(["aarch64-apple-darwin", "x86_64-apple-darwin"]);
    for (const [triple, sha] of Object.entries(pins)) {
      expect(sha, `pin for ${triple} must be a 40-char lowercase hex sha`).toMatch(/^[0-9a-f]{40}$/);
    }
    expect(pins["aarch64-apple-darwin"]).not.toBe(pins["x86_64-apple-darwin"]);
    expect(
      pins["aarch64-apple-darwin"],
      "arm pin: newest commit carrying vips 8.18.7 + gs 10.08.0 arm64_sequoia bottles (ADR 0001)",
    ).toBe("cb5b5e2230fdc721d35ad6ce88f096ca628b2abf");
    expect(
      pins["x86_64-apple-darwin"],
      "x86 pin: parent of the openssl@3 3.6.4 bump — the first 2026-08/09 homebrew-core commit on the vips 8.18.6 + gs 10.07.1 window whose whole 61-formula macOS closure (incl. cgif 0.5.3, libheif 1.23.2) still carries sonoma x86_64 bottles; its successor commits lose one (cgif 0.5.4 on 09-02, libheif 1.23.3 on 09-01) and never regain one (ADR 0001)",
    ).toBe("c36952de43157c2c4a4ea05f385031aa29f30354");
  });

  it("the manifest freezes Intel on the last sonoma-bottled brew engines — newer versions have no x86_64 bottle ever", async () => {
    const manifest = JSON.parse(
      await readFile(new URL("../../scripts/binaries.manifest.json", import.meta.url), "utf8"),
    );
    const arm = manifest.targets["aarch64-apple-darwin"];
    const x86 = manifest.targets["x86_64-apple-darwin"];
    expect(x86.vips.source).toBe("brew");
    expect(x86.gs.source).toBe("brew");
    expect(
      x86.vips.expectVersion,
      "vips 8.18.7 first entered the tap 16 days after the last x86_64 bottle was published — Intel must stay on 8.18.6 until a human re-verifies bottles (ADR 0001)",
    ).toBe("8.18.6");
    expect(
      x86.gs.expectVersion,
      "ghostscript 10.08.0 bottles postdate the x86_64 cutoff — Intel must stay on 10.07.1 until a human re-verifies bottles (ADR 0001)",
    ).toBe("10.07.1");
    expect(arm.vips.expectVersion).toBe("8.18.7");
    expect(arm.gs.expectVersion).toBe("10.08.0");
  });

  it("brew formulas install off the pinned tap with auto-update and API resolution disabled", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    const brew = stepsOf(freeze).find((s) => stepRuns(s).includes("brew install vips ghostscript"));
    expect(brew, "brew install step must exist (drift gate needs the active keg)").toBeDefined();
    expect(
      brew.run,
      "the formula install must be naked — drift handling belongs to prep/verify, not to a version flag here",
    ).not.toContain("@");
    expect(brew.env?.HOMEBREW_NO_AUTO_UPDATE, "auto-update would wipe the pin checkout with fresh core").toBe("1");
    expect(brew.env?.HOMEBREW_NO_INSTALL_FROM_API, "API resolution would bypass the pinned tap formulas").toBe("1");
    expect(brew["if"]).toBe("matrix.platform == 'mac'");
    expect(brew.shell).toBe("bash");
  });

  it("no brew update step exists anywhere (Ruling: replaced by the homebrew-core pin checkout)", async () => {
    const { doc, text } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    expect(
      stepsOf(freeze).find((s) => stepRuns(s).includes("brew update")),
      "brew update would pull HEAD homebrew-core, whose formula set cannot match the pin — removed by Ruling",
    ).toBeUndefined();
    expect(text.includes("brew update")).toBe(false);
  });

  it("freeze job steps follow the acceptance-criteria order", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    const steps = stepsOf(freeze);
    const uses = steps.map(stepUses);

    const checkoutIdx = uses.findIndex((u) => u.startsWith("actions/checkout@"));
    const setupNodeIdx = uses.findIndex((u) => u.startsWith("actions/setup-node@"));
    const toolchainIdx = uses.findIndex((u) => u.startsWith("dtolnay/rust-toolchain@"));
    const uploadIdx = uses.findIndex((u) => u.startsWith("actions/upload-artifact@"));
    const installIdx = runIndexesByText(freeze, "yarn install --frozen-lockfile")[0];
    const pinIdx = steps.findIndex((s) => stepRuns(s).includes("scripts/ci/homebrew-core-pins.json"));
    const brewIdx = runIndexesByText(freeze, "brew install vips ghostscript")[0];
    const allIdx = runIndexesByText(freeze, "node scripts/binaries/all.mjs --target")[0];
    const freezeIdx = runIndexesByText(freeze, "bash scripts/ci/freeze.sh --triple")[0];

    const observed = [checkoutIdx, setupNodeIdx, installIdx, pinIdx, brewIdx, toolchainIdx, allIdx, freezeIdx, uploadIdx];
    for (const [i, idx] of observed.entries()) {
      expect(idx, `step group #"${i}" (checkout/setup-node/install/pin/brew/toolchain/binaries/freeze/upload) is present`).toBeGreaterThanOrEqual(0);
    }
    for (let i = 1; i < observed.length; i++) {
      expect(observed[i], `step group ${i} out of acceptance-criteria order`).toBeGreaterThan(observed[i - 1]);
    }
  });

  it("staging and bundling run for every row with the matrix triple", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;

    const staging = stepsOf(freeze).find((s) =>
      stepRuns(s).includes("node scripts/binaries/all.mjs --target"),
    );
    expect(staging).toBeDefined();
    expect(staging["if"], "staging is identical in shape on all four rows").toBeUndefined();
    expect(staging.env?.TRIPLE).toBe("${{ matrix.triple }}");

    const freezeStep = stepsOf(freeze).find((s) => stepRuns(s).includes("bash scripts/ci/freeze.sh --triple"));
    expect(freezeStep).toBeDefined();
    expect(freezeStep["if"], "bundling is identical in shape on all four rows").toBeUndefined();
    expect(freezeStep.env?.TRIPLE).toBe("${{ matrix.triple }}");
  });

  it("only mac rows carry rust toolchain (windows rows stage from URL pins, no rustc needed for bundling)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const toolchain = stepsOf(jobsOf(doc).freeze).find((s) =>
      stepUses(s).startsWith("dtolnay/rust-toolchain@"),
    );
    expect(toolchain).toBeDefined();
    expect(toolchain["if"]).toBe("matrix.platform == 'mac'");
  });

  it("setup-node pins node 22 with yarn cache", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const setupNode = stepsOf(jobsOf(doc).freeze).find((s) => stepUses(s).startsWith("actions/setup-node@"));
    expect(setupNode.with["node-version"]).toBe(22);
    expect(setupNode.with.cache).toBe("yarn");
  });

  it("upload-artifact carries the bundle + sha256 for this matrix row and errors on no files", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const upload = stepsOf(jobsOf(doc).freeze).find((s) => stepUses(s).startsWith("actions/upload-artifact@"));
    const withBlock = JSON.stringify(upload.with ?? {});
    expect(withBlock).toContain("src-tauri/.freeze/sidecar-bundle-");
    expect(withBlock).toContain(".tar.gz");
    expect(withBlock).toContain(".sha256");
    expect(upload["with"].name).toContain("${{ matrix.triple }}");
    expect(upload["with"]["if-no-files-found"]).toBe("error");
  });

  it("every run: step pins shell: bash", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        if (!stepRuns(step)) continue;
        expect(step.shell, `job ${jobName} step "${step.name}": run steps must set shell: bash`).toBe("bash");
      }
    }
  });

  it("gate job runs a fail-closed tag-collision preflight before the expensive freeze matrix", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const gate = jobsOf(doc).gate;
    expect(gate, "gate job must exist").toBeDefined();
    expect(gate.needs).toBeUndefined();
    expect(gate["runs-on"]).toBe("ubuntu-24.04");
    expect(jobsOf(doc).freeze.needs).toEqual(["gate"]);
    expect(
      stepsOf(jobsOf(doc).finalize).find((s) => stepRuns(s).includes("collision")),
      "collision preflight must live only in the gate job",
    ).toBeUndefined();
    expect(
      stepsOf(jobsOf(doc).finalize).find((s) => stepRuns(s).includes("2>&1 >/dev/null)\" || rc=$?")),
      "stderr-classified collision probe must not run in finalize",
    ).toBeUndefined();

    const collision = stepsOf(gate).find((s) => stepRuns(s).includes('gh release view "$tag"'));
    expect(collision, "collision preflight must query gh release view").toBeDefined();
    expect(collision.name).toBe("report same-day sidecars release collision with remediation");
    expect(collision.run).toContain("set -euo pipefail");
    expect(collision.run).toContain('tag="sidecars-$(date -u +%Y%m%d)"');
    expect(collision.run, "tag must not be recomputed inline").not.toContain('gh release view "sidecars-');
    expect(collision.run, "stderr must be captured and classified (fail-closed)").toContain(
      'err="$(gh release view "$tag" 2>&1 >/dev/null)" || rc=$?',
    );
    expect(collision.run).toContain(
      "::error::collision preflight could not verify tag absence (auth/scopes/rate limit/network): $err",
    );
    expect(collision.run, "only genuine 'not found' stderr may pass the gate").toMatch(/not found\|no release/i);
    expect(collision.run).toContain(
      "release $tag already exists — recovery: delete any leftover draft release, run 'gh release delete $tag --cleanup-tag --yes', then re-run the failed freeze jobs",
    );
    expect(collision.env?.GH_REPO).toBe("${{ github.repository }}");
    expect(collision.env?.GH_TOKEN).toBe("${{ github.token }}");
  });

  it("windows rows dispatch the freeze script in bash (survives the git-bash runner shell)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freezeStep = stepsOf(jobsOf(doc).freeze).find((s) =>
      stepRuns(s).includes("bash scripts/ci/freeze.sh --triple"),
    );
    expect(freezeStep, "freeze step must exist").toBeDefined();
    expect(freezeStep.run).toContain("bash scripts/ci/freeze.sh");
    expect(freezeStep.run, "the triple argument must come from env on every row").not.toContain("${{");
  });

  it("no run: step in any job interpolates raw ${{ }} templates", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        const runText = stepRuns(step);
        if (!runText) continue;
        expect(
          runText.includes("${{"),
          `job ${jobName} step "${step.name}": run scripts must not embed raw templates — pass context through step env`,
        ).toBe(false);
      }
    }
  });

  it("finalize job needs freeze and downloads bundles", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const finalize = jobsOf(doc).finalize;
    expect(finalize).toBeDefined();
    expect(finalize.needs).toEqual(["freeze"]);

    const steps = stepsOf(finalize);
    const downloadIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/download-artifact@"));
    expect(downloadIdx, "finalize must download artifacts").toBeGreaterThanOrEqual(0);
  });

  it("finalize downloads artifacts with the sidecar-bundle pattern, merged into release-assets", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const download = stepsOf(jobsOf(doc).finalize).find((s) =>
      stepUses(s).startsWith("actions/download-artifact@"),
    );
    expect(download.with.pattern).toBe("sidecar-bundle-*");
    expect(download.with["merge-multiple"]).toBe(true);
    expect(download.with.path).toBe("release-assets");
  });

  it("finalize count preflight requires all eight bundle assets across the four triples", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).finalize);

    const downloadIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/download-artifact@"));
    const countIdx = steps.findIndex((s) => stepRuns(s).includes("expected 8 bundle assets (4 tarballs + 4 .sha256)"));
    const createIdx = steps.findIndex((s) => stepRuns(s).includes('gh release create "$tag"'));

    expect(countIdx, "asset-count preflight step must exist").toBeGreaterThanOrEqual(0);
    expect(createIdx, "publish step must exist").toBeGreaterThanOrEqual(0);
    expect(countIdx, "count preflight must run after download").toBeGreaterThan(downloadIdx);
    expect(createIdx, "release must only be created after the count preflight").toBeGreaterThan(countIdx);

    const countStep = steps[countIdx];
    expect(countStep.run).toContain("set -euo pipefail");
    expect(countStep.run, "missing dir must fail loudly before find runs").toContain("[ ! -d release-assets ]");
    expect(countStep.run).toContain(
      "::error::release-assets directory missing — download-artifact step produced nothing",
    );
    expect(countStep.run).toContain('[ "$count" -ne 8 ]');
    expect(countStep.shell).toBe("bash");
  });

  it("finalize publishes draft-first (draft → upload → verify 8 assets retried → publish) in one step", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const publish = stepsOf(jobsOf(doc).finalize).find((s) => stepRuns(s).includes('gh release create "$tag"'));
    expect(publish, "publish step must exist").toBeDefined();
    const run = publish.run;

    expect(run).toContain("set -euo pipefail");
    expect(run).toContain('tag="sidecars-$(date -u +%Y%m%d)"');
    expect(run, "tag must not be recomputed inline").not.toContain('gh release create "sidecars-');
    expect(run).toContain('gh release create "$tag" --draft --target "$GITHUB_SHA" --title "$tag"');
    expect(run, "assets must not be uploaded as part of create (torn-publish risk)").not.toContain(
      'gh release create "$tag" release-assets',
    );
    expect(run).toContain('gh release upload "$tag" release-assets/* --clobber');
    expect(run).toContain("n=\"$(gh release view \"$tag\" --json assets --jq '.assets | length')\"");
    expect(run).toContain('[ "$n" -ne 8 ]');
    expect(run).toContain("::error::$tag has $n/8 assets — draft left in place, not published");
    expect(run).toContain("gh release delete $tag --cleanup-tag --yes");
    expect(run).toContain('gh release edit "$tag" --draft=false');
    expect(publish.env?.GH_REPO).toBe("${{ github.repository }}");
    expect(publish.env?.GH_TOKEN).toBe("${{ github.token }}");

    const order = [
      run.indexOf('gh release create "$tag" --draft'),
      run.indexOf('gh release upload "$tag" release-assets/* --clobber'),
      run.indexOf("--json assets --jq '.assets | length'"),
      run.indexOf('gh release edit "$tag" --draft=false'),
    ];
    for (const [i, idx] of order.entries()) {
      expect(idx, `draft-first stage #${i} (create/upload/verify/publish) missing`).toBeGreaterThanOrEqual(0);
    }
    for (let i = 1; i < order.length; i++) {
      expect(order[i], `draft-first stage ${i} out of order`).toBeGreaterThan(order[i - 1]);
    }

    const loopIdx = run.indexOf("for attempt in 1 2 3 4");
    expect(loopIdx, "the verify must retry the asset read: 4 attempts with a gap between them").toBeGreaterThanOrEqual(0);
    const countedIdx = run.indexOf("n=\"$(gh release view \"$tag\" --json assets --jq '.assets | length')\"");
    expect(countedIdx, "the counted asset query must run inside the retry loop").toBeGreaterThan(loopIdx);
    const namesIdx = run.indexOf("names=\"$(gh release view \"$tag\" --json assets --jq '.assets[].name')\"");
    expect(namesIdx, "the exact-8-name set check must run in the same retried pass as the count").toBeGreaterThan(countedIdx);
    expect(
      run,
      "the expected-8-name loop must iterate all four triples",
    ).toContain("for t in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc aarch64-pc-windows-msvc");
    expect(run, "each expected tarball name must be asserted present in the retried pass").toContain(
      'grep -Fxq "sidecar-bundle-$t.tar.gz"',
    );
    expect(run, "each expected digest name must be asserted present in the retried pass").toContain(
      'grep -Fxq "sidecar-bundle-$t.tar.gz.sha256"',
    );
    const guardIdx = run.indexOf("if [ \"$attempt\" -lt 4 ]; then");
    const sleepIdx = run.indexOf("sleep 10");
    expect(guardIdx, "the inter-attempt gap must be guarded so the final attempt does not sleep").toBeGreaterThan(namesIdx);
    expect(sleepIdx).toBeGreaterThan(guardIdx);
    expect(
      run.indexOf("::error::$tag has $n/8 assets — draft left in place, not published"),
      "the mismatch annotation must fire only after the retry loop is exhausted",
    ).toBeGreaterThan(sleepIdx);
  });

  it("finalize is idempotent for re-run failed jobs: an existing draft at the tag is reused and a published one fails loud", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const publish = stepsOf(jobsOf(doc).finalize).find((s) => stepRuns(s).includes('gh release create "$tag"'));
    expect(publish, "publish step must exist").toBeDefined();
    const run = publish.run;

    const probeIdx = run.indexOf("draft=\"$(gh release view \"$tag\" --json isDraft --jq '.isDraft' 2>/dev/null)\"");
    expect(probeIdx, "finalize must probe the existing tag state before gh release create").toBeGreaterThanOrEqual(0);
    const createIdx = run.indexOf('gh release create "$tag" --draft');
    expect(createIdx, "gh release create must be decided after the tag-state probe").toBeGreaterThan(probeIdx);
    const reuseIdx = run.indexOf("reusing existing draft $tag");
    expect(reuseIdx, "reusing an existing draft must be logged").toBeGreaterThan(probeIdx);
    expect(reuseIdx, "the reuse branch must sit inside the probe's if, before the create falls into its else").toBeLessThan(createIdx);
    expect(run, "a published release at the tag must fail loud, never be reused").toContain(
      "::error::release $tag already exists and is published",
    );
    expect(run, "the published-release error must carry the delete-and-re-run recovery").toContain(
      "gh release delete $tag --cleanup-tag --yes",
    );
  });

  it("mac rows derive the tauri.conf.json frameworks list from the staged tree after staging; windows rows skip it", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const freeze = jobsOf(doc).freeze;
    const steps = stepsOf(freeze);
    const sync = steps.find((s) => stepRuns(s).includes("scripts/binaries/sync-macos-frameworks.mjs"));
    expect(sync, "a frameworks-sync step must exist on freeze rows").toBeDefined();
    expect(sync["if"], "frameworks are a macOS-only artifact — the sync must gate on the mac platform").toBe(
      "matrix.platform == 'mac'",
    );
    expect(sync.env?.TRIPLE).toBe("${{ matrix.triple }}");
    expect(
      sync.run,
      "the sync is resolved from the row's triple env — never a workflow expression inside the run string",
    ).toContain('node scripts/binaries/sync-macos-frameworks.mjs --target "$TRIPLE"');

    const stagingIdx = runIndexesByText(freeze, "node scripts/binaries/all.mjs --target")[0];
    const syncIdx = steps.findIndex((s) => s === sync);
    const freezeIdx = runIndexesByText(freeze, "bash scripts/ci/freeze.sh --triple")[0];
    expect(stagingIdx).toBeGreaterThanOrEqual(0);
    expect(syncIdx, "the sync must run after the all.mjs staging step").toBeGreaterThan(stagingIdx);
    expect(freezeIdx, "the sync must run before the bundle freezes the tree").toBeGreaterThan(syncIdx);
  });
});

describe("release.yml release workflow", () => {
  const FILENAME = "release.yml";

  it("has a self-check status of current", () => {
    expect(EXPECTED_WORKFLOWS.find((wf) => wf.file === FILENAME)?.status).toBe("current");
  });

  it("triggers on push of v* tags and workflow_dispatch with a tag input", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.on.push.tags).toEqual(["v*"]);
    expect(doc.on.push.branches).toBeUndefined();
    const dispatch = doc.on.workflow_dispatch;
    expect(dispatch, "workflow_dispatch must be configured").toBeTypeOf("object");
    expect(dispatch.inputs?.tag?.type, "dispatch must carry a string tag input").toBe("string");
  });

  it("queues concurrent runs per computed release tag (concurrency, cancel-in-progress: false)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.concurrency.group).toBe(
      "release-${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref }}",
    );
    expect(doc.concurrency["cancel-in-progress"]).toBe(false);
  });

  it("scopes permissions: workflow reads, gate writes only for the draft, rows write contents", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.permissions).toEqual({ contents: "read" });
    expect(
      jobsOf(doc).gate.permissions,
      "creating the draft is the gate's single write op — a contents:read token 403s on gh release create",
    ).toEqual({ contents: "write" });
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      if (jobName === "gate") continue;
      expect(job.permissions, `job ${jobName} must hold job-scoped contents: write`).toEqual({
        contents: "write",
      });
    }
  });

  it("has exactly the native 2x2 four-runner matrix (mac x86 builds natively on macos-15-intel)", async () => {
    const { doc, text } = await loadWorkflow(FILENAME);
    const release = jobsOf(doc).release;
    expect(release).toBeDefined();
    expect(release["timeout-minutes"], "release rows must be bounded at 90 — cargo builds are slow but not 6-hour hangs").toBe(90);
    expect(release.strategy["fail-fast"]).toBe(false);
    expect(release.strategy.matrix.include).toEqual(RELEASE_MATRIX_ROWS);

    const matrixText = JSON.stringify(release.strategy.matrix);
    expect(matrixText, "all four rows are consumers now — no stage field remains").not.toContain("stage");
    expect(
      text,
      "x86 mac builds natively on macos-15-intel — no cross-compile target add for it (Ruling)",
    ).not.toContain("rustup target add x86_64-apple-darwin");
  });

  it("bounds every job: cheap gate 15, release rows 90", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(jobsOf(doc).gate["timeout-minutes"]).toBe(15);
    expect(jobsOf(doc).release["timeout-minutes"]).toBe(90);
  });

  it("version-sync uses trigger-context env instead of run-string interpolation", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const sync = stepsOf(jobsOf(doc).gate).find((s) =>
      stepRuns(s).includes("node scripts/ci/release-version-check.mjs"),
    );
    expect(sync, "gate version-sync step must exist").toBeDefined();
    expect(sync.env?.GITHUB_EVENT_NAME).toBe("${{ github.event_name }}");
    expect(sync.env?.GITHUB_REF_NAME).toBe("${{ github.ref_name }}");
    expect(sync.env?.DISPATCH_TAG).toBe("${{ inputs.tag }}");
    expect(sync.run).toContain("node scripts/ci/release-version-check.mjs");
    expect(sync.run, "untrusted fragments must enter through env, not the script text").not.toContain(
      "${{",
    );
    expect(sync.shell).toBe("bash");

    const checkoutIdx = stepsOf(jobsOf(doc).gate).findIndex((s) => stepUses(s).startsWith("actions/checkout@"));
    const syncIdx = stepsOf(jobsOf(doc).gate).findIndex((s) => stepRuns(s).includes("node scripts/ci/release-version-check.mjs"));
    expect(checkoutIdx, "checkout must precede version-sync").toBeLessThan(syncIdx);
  });

  it("no run: step in any job interpolates raw ${{ }} templates", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        const runText = stepRuns(step);
        if (!runText) continue;
        expect(
          runText.includes("${{"),
          `job ${jobName} step "${step.name}": run scripts must not embed raw templates — pass context through step env`,
        ).toBe(false);
      }
    }
  });

  it("gate job steps: cheap version + sidecars + draft checks, ordered before the matrix", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).gate);
    const checkoutIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/checkout@"));
    const setupNodeIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/setup-node@"));
    const syncIdx = steps.findIndex((s) => stepRuns(s).includes("node scripts/ci/release-version-check.mjs"));
    const preflightIdx = steps.findIndex((s) => stepRuns(s).includes("sidecars-availability"));
    const draftIdx = steps.findIndex((s) => stepRuns(s).includes('gh release create "$RELEASE_TAG"'));

    expect(checkoutIdx).toBeGreaterThanOrEqual(0);
    expect(setupNodeIdx, "node must be set up before the helper runs").toBeGreaterThanOrEqual(0);
    expect(syncIdx, "version-sync step must exist").toBeGreaterThanOrEqual(0);
    expect(preflightIdx, "sidecars-availability preflight must be a real step").toBeGreaterThanOrEqual(0);
    expect(draftIdx, "draft pre-creation step must exist").toBeGreaterThanOrEqual(0);
    for (let i = 1; i < [checkoutIdx, setupNodeIdx, syncIdx, preflightIdx, draftIdx].length; i++) {
      const idx = [checkoutIdx, setupNodeIdx, syncIdx, preflightIdx, draftIdx][i];
      expect(idx, `gate step group ${i} out of order (cheap fail-fast before the 30-min matrix)`).toBeGreaterThan(
        [checkoutIdx, setupNodeIdx, syncIdx, preflightIdx, draftIdx][i - 1],
      );
    }
    expect(setupNodeIdx).toBeLessThan(syncIdx);
  });

  it("gate preflights sidecars availability for all four triples with all eight bundle assets", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const preflight = stepsOf(jobsOf(doc).gate).find((s) => stepRuns(s).includes("sidecars-"));
    expect(preflight, "sidecars-availability preflight must exist").toBeDefined();
    const run = preflight.run;
    expect(run).toContain("set -euo pipefail");
    expect(run).toContain("gh release list --limit 200 --json tagName");
    expect(run).toContain('startswith("sidecars-")');
    expect(
      run,
      "a failed release list must fail closed with its own annotation, not collapse into 'no sidecars release'",
    ).toContain("::error::sidecars-availability preflight could not list releases (auth/scopes/rate limit/network)");
    expect(run).toContain("::error::no sidecars-* release found — run the freeze.yml snapshot workflow before building the release (ADR 0001), then re-run the release");
    for (const triple of ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]) {
      expect(run).toContain(triple);
    }
    for (const pattern of ["sidecar-bundle-${triple}.tar.gz", "sidecar-bundle-${triple}.tar.gz.sha256"]) {
      expect(run).toContain(pattern);
    }
    expect(run).toContain("::error::sidecars release '$newest' is missing asset");
    expect(run).toContain("all eight bundle assets present on");
    expect(preflight.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(preflight.env?.GH_REPO).toBe("${{ github.repository }}");
    expect(preflight.shell).toBe("bash");
  });

  it("gate pre-creates the draft release fail-closed and exports job outputs", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const gate = jobsOf(doc).gate;
    expect(gate.outputs).toEqual({
      release_id: "${{ steps.draft.outputs.release_id }}",
      release_tag: "${{ steps.draft.outputs.release_tag }}",
      app_version: "${{ steps.draft.outputs.app_version }}",
      prerelease: "${{ steps.draft.outputs.prerelease }}",
    });

    const draft = stepsOf(gate).find((s) => stepRuns(s).includes('gh release create "$RELEASE_TAG"'));
    expect(draft, "draft creation must live in the gate job").toBeDefined();
    expect(draft.id).toBe("draft");
    const run = draft.run;

    expect(run).toContain("set -euo pipefail");
    expect(run, "leftover drafts or published releases must fail the gate loudly").toContain(
      "::error::release $RELEASE_TAG already exists — recovery: run 'gh release delete $RELEASE_TAG --cleanup-tag --yes', then re-run the release",
    );
    expect(run).toContain('err="$(gh release view "$RELEASE_TAG" 2>&1 >/dev/null)" || rc=$?');
    expect(run, "stderr must be captured and classified (fail-closed)").toMatch(/not found\|no release/i);

    expect(run).toContain('gh release create "$RELEASE_TAG" "${draft_flags[@]}" >/dev/null');
    expect(run).toContain("draft_flags=(--draft --target \"$GITHUB_SHA\" --title \"Yard $RELEASE_TAG\")");
    expect(run, "the prerelease flag must be decided from the helper env, not recomputed").toContain(
      '[ "$PRERELEASE" = "true" ]',
    );
    expect(run).toContain("draft_flags+=(--prerelease)");
    expect(run).toContain('release_id="$(gh release view "$RELEASE_TAG" --json databaseId --jq \'.databaseId\')"');
    expect(
      run,
      "the id must be verified numeric before it reaches tauri-action — Number('R_...') is NaN and NaN is falsy, so a node-id would silently skip every upload",
    ).toContain('case "$release_id" in');
    expect(run).toContain(
      "::error::gate resolved a non-numeric release id ('$release_id') — tauri-action needs the numeric REST id (databaseId); re-run the release",
    );
    const guardIdx = run.indexOf('case "$release_id" in');
    const outputIdx = run.indexOf(">> \"$GITHUB_OUTPUT\"");
    expect(guardIdx, "the numeric guard must hold before the release id is exported to rows").toBeGreaterThanOrEqual(0);
    expect(outputIdx).toBeGreaterThanOrEqual(0);
    expect(guardIdx).toBeLessThan(outputIdx);
    for (const output of ["release_id", "release_tag", "app_version", "prerelease"]) {
      expect(run).toContain(`echo "${output}=`);
    }
    expect(draft.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(draft.env?.GH_REPO).toBe("${{ github.repository }}");
    expect(draft.shell).toBe("bash");
  });

  it("rows re-run the version check validate-only before engine work", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const checkoutIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/checkout@"));
    const syncIdx = steps.findIndex((s) => stepRuns(s).includes("--validate-only"));
    expect(syncIdx, "row validate-only version check must exist").toBeGreaterThanOrEqual(0);
    expect(checkoutIdx, "checkout must precede the row version check").toBeLessThan(syncIdx);
    const sync = steps[syncIdx];
    expect(sync.run).toContain("node scripts/ci/release-version-check.mjs --validate-only");
    expect(sync.env?.GITHUB_EVENT_NAME).toBe("${{ github.event_name }}");
    expect(sync.env?.DISPATCH_TAG).toBe("${{ inputs.tag }}");
    expect(sync.shell).toBe("bash");
  });

  it("runs the gate command verbatim after engine restoration", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const release = jobsOf(doc).release;
    const gateIdx = runIndexesByText(release, GATE_COMMAND)[0];
    expect(gateIdx, "gate command must appear verbatim").toBeGreaterThanOrEqual(0);
    const steps = stepsOf(release);
    const auditIdx = steps.findIndex((s) => stepRuns(s).includes("yarn binaries:audit"));
    expect(auditIdx, "engine audit must precede the gate").toBeLessThan(gateIdx);
  });

  it("mac rows allow the inactive brew version-drift gate on the frozen-engine audit", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const audit = stepsOf(jobsOf(doc).release).find((s) => stepRuns(s).includes("yarn binaries:audit"));
    expect(audit, "frozen-engine audit step must exist").toBeDefined();
    expect(audit["if"], "the allowance is scoped to the mac rows via runner.os").toBe("runner.os == 'macOS'");
    expect(
      audit.env?.BINARIES_ALLOW_INACTIVE_BREW,
      "runner brew has no vips/ghostscript kegs — the inactive allowance is what keeps the audit from dying on brew-present/formula-missing hosts",
    ).toBe("1");
  });

  it("every row unfreezes from sidecars-* and audits — no manifest staging anywhere", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const release = jobsOf(doc).release;
    const steps = stepsOf(release);

    const unfreezeStep = steps.find((s) => stepRuns(s).includes("bash scripts/ci/unfreeze.sh --triple"));
    expect(unfreezeStep, "unfreeze step must exist").toBeDefined();
    expect(unfreezeStep.run).toContain('--triple "$TRIPLE"');
    expect(unfreezeStep.env?.TRIPLE).toBe("${{ matrix.triple }}");
    expect(unfreezeStep["if"], "all four rows are consumers now — no stage conditioning remains").toBeUndefined();
    expect(unfreezeStep.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(unfreezeStep.env?.GH_REPO).toBe("${{ github.repository }}");

    const audits = steps.filter((s) => stepRuns(s).includes("yarn binaries:audit"));
    expect(audits, "one audit step per runner os").toHaveLength(2);
    const mac = audits.find((s) => s["if"] === "runner.os == 'macOS'");
    const win = audits.find((s) => s["if"] === "runner.os == 'Windows'");
    expect(mac, "mac audit step must exist").toBeDefined();
    expect(win, "windows audit step must exist").toBeDefined();
    expect(mac.env?.BINARIES_ALLOW_INACTIVE_BREW).toBe("1");
    expect(win.env?.BINARIES_ALLOW_INACTIVE_BREW, "windows rows must not carry the mac brew allowance").toBeUndefined();

    const unfreezeIdx = steps.findIndex((s) => s === unfreezeStep);
    for (const audit of audits) {
      expect(steps.findIndex((s) => s === audit), "audit must run after unfreeze").toBeGreaterThan(unfreezeIdx);
    }

    const syncStep = steps.find((s) => stepRuns(s).includes("scripts/binaries/sync-macos-frameworks.mjs"));
    expect(syncStep, "the restored mac bundle carries tauri.conf.json dylib entries that the tree's actual dylibs must re-derive before the audit").toBeDefined();
    expect(syncStep["if"]).toBe("runner.os == 'macOS'");
    expect(syncStep.env?.TRIPLE).toBe("${{ matrix.triple }}");
    expect(syncStep.run).toContain('node scripts/binaries/sync-macos-frameworks.mjs --target "$TRIPLE"');
    expect(syncStep.shell).toBe("bash");
    const syncIdx = steps.findIndex((s) => s === syncStep);
    for (const audit of audits) {
      expect(steps.findIndex((s) => s === audit), "audit must run after the mac frameworks sync").toBeGreaterThan(syncIdx);
    }

    const runsText = steps.map((s) => stepRuns(s)).join("\n");
    for (const row of RELEASE_MATRIX_ROWS) {
      expect(
        runsText.includes('bash scripts/ci/unfreeze.sh --triple "$TRIPLE"'),
        `row ${row.triple}: unfreeze must be invoked`,
      ).toBe(true);
    }
    expect(runsText, "consumer rows never stage via all.mjs").not.toContain("all.mjs");
  });

  it("is a pure consumer: no brew, no all.mjs staging, no vendor fetch anywhere in the workflow", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        const runText = stepRuns(step);
        if (!runText) continue;
        expect(runText, `job ${jobName} step "${step.name}": consumers never invoke brew`).not.toMatch(/brew/i);
        expect(runText, `job ${jobName} step "${step.name}": consumers never stage via all.mjs`).not.toContain("all.mjs");
        expect(runText, `job ${jobName} step "${step.name}": consumers never vendor engines locally`).not.toMatch(/vendor/i);
      }
    }
  });

  it("arm windows row adds its rustup target after toolchain setup", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const toolchainIdx = steps.findIndex((s) => stepUses(s).startsWith("dtolnay/rust-toolchain@"));
    const rustupIdx = steps.findIndex((s) => stepRuns(s).includes("rustup target add aarch64-pc-windows-msvc"));
    expect(toolchainIdx).toBeGreaterThanOrEqual(0);
    expect(rustupIdx, "rustup target add step must exist").toBeGreaterThanOrEqual(0);
    expect(rustupIdx).toBeGreaterThan(toolchainIdx);
    const rustup = steps[rustupIdx];
    expect(rustup["if"]).toContain("aarch64-pc-windows-msvc");
    expect(rustup.shell).toBe("bash");
  });

  it("uses rust-cache keyed per triple over the src-tauri workspace", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const cache = stepsOf(jobsOf(doc).release).find((s) =>
      /swatinem\/rust-cache@/i.test(stepUses(s)),
    );
    expect(cache, "rust-cache step must exist").toBeDefined();
    expect(cache.with.key).toContain("${{ matrix.triple }}");
    expect(cache.with.workspaces).toBe("src-tauri");
  });

  it("every run: step pins shell: bash", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        if (!stepRuns(step)) continue;
        expect(step.shell, `job ${jobName} step "${step.name}": run steps must set shell: bash`).toBe("bash");
      }
    }
  });

  it("setup-node pins node 22 with yarn cache on the release rows", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const setupNode = stepsOf(jobsOf(doc).release).find((s) => stepUses(s).startsWith("actions/setup-node@"));
    expect(setupNode.with["node-version"]).toBe(22);
    expect(setupNode.with.cache).toBe("yarn");
  });

  it("tauri-action uploads into the gated draft by releaseId, with no tag/name/prerelease inputs", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const action = stepsOf(jobsOf(doc).release).find((s) =>
      stepUses(s).startsWith("tauri-apps/tauri-action@"),
    );
    expect(action, "tauri-action step must exist").toBeDefined();
    expect(stepUses(action)).toMatch(/^tauri-apps\/tauri-action@[0-9a-f]{40}$/);
    expect(action.env?.GITHUB_TOKEN).toBe("${{ github.token }}");

    const withBlock = action.with ?? {};
    expect(withBlock.releaseId, "per tauri-action semantics at a6e90dd: releaseId wins, tagName/releaseName are ignored").toBe(
      "${{ needs.gate.outputs.release_id }}",
    );
    for (const dropped of ["tagName", "releaseName", "releaseBody", "prerelease"]) {
      expect(
        withBlock[dropped],
        `tauri-action input '${dropped}' must not be set — release creation is the gate's job and duplicate drafts were the known tauri-action race`,
      ).toBeUndefined();
    }
    expect(withBlock.releaseDraft).toBe(true);
    expect(withBlock.args).toBe("--target ${{ matrix.triple }}");
  });

  it("uploads the stable README download aliases plus SHA-256 checksums after tauri-action", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const actionIdx = steps.findIndex((s) => stepUses(s).startsWith("tauri-apps/tauri-action@"));
    const aliasStep = steps.find((s) => s.name?.startsWith("upload stable download alias"));
    expect(aliasStep, "stable alias upload step must exist").toBeDefined();
    expect(steps.findIndex((s) => s === aliasStep)).toBeGreaterThan(actionIdx);

    const run = aliasStep.run;
    for (const [triple, alias] of [
      ["aarch64-apple-darwin", "Yard-macOS-arm64.dmg"],
      ["x86_64-apple-darwin", "Yard-macOS-x64.dmg"],
      ["x86_64-pc-windows-msvc", "Yard-Windows-x64-setup.exe"],
      ["aarch64-pc-windows-msvc", "Yard-Windows-arm64-setup.exe"],
    ]) {
      expect(run, `${triple} must publish ${alias}`).toContain(`alias="${alias}"`);
    }
    expect(run).toContain('-name "*${APP_VERSION}*"');
    expect(run).toContain('cp "$asset" "$RUNNER_TEMP/$alias"');
    expect(run).toContain('gh release upload "$RELEASE_TAG" "$RUNNER_TEMP/$alias" --clobber');
    expect(run).toContain('printf \'%s  %s\\n\' "$digest" "$alias"');
    expect(run).toContain('gh release upload "$RELEASE_TAG" "$RUNNER_TEMP/$alias.sha256" --clobber');
    expect(aliasStep.env?.APP_VERSION).toBe("${{ needs.gate.outputs.app_version }}");
    expect(aliasStep.env?.TRIPLE).toBe("${{ matrix.triple }}");
  });

  it("fail-closes each row when the gated draft carries no app artifacts for its triple after tauri-action", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const actionIdx = steps.findIndex((s) => stepUses(s).startsWith("tauri-apps/tauri-action@"));
    const check = steps.find((s) => stepRuns(s).includes('grep -Eic -- "$pattern"'));
    expect(check, "per-row app-asset post-upload check must exist").toBeDefined();
    const checkIdx = steps.findIndex((s) => s === check);
    expect(checkIdx, "the app-asset check must run after tauri-action").toBeGreaterThan(actionIdx);
    expect(check["if"], "the app-asset check must run on every matrix row").toBeUndefined();

    const run = check.run;
    expect(run).toContain("set -euo pipefail");
    for (const { triple, pattern } of APP_ASSET_PATTERNS) {
      expect(run, `${triple} must map to the tauri-bundler asset-name pattern '${pattern}'`).toContain(
        `${triple}) pattern='${pattern}' ;;`,
      );
    }
    expect(
      run,
      "an unmapped triple must fail loudly instead of passing the row",
    ).toContain("::error::no app-asset pattern implemented for triple '$TRIPLE' — extend the mapping table in the release workflow");
    expect(run).toContain('names="$(gh release view "$RELEASE_TAG" --json assets --jq \'.assets[].name\')"');
    expect(
      run,
      "a failed asset read must be classified fail-closed before the count runs",
    ).toContain("::error::app-asset check could not read the draft assets of $RELEASE_TAG (auth/scopes/rate limit/network)");
    expect(
      run,
      "grep exits 1 on no match under pipefail — the count must default to 0 so an empty catch cannot read as success",
    ).toContain('count="$(printf \'%s\\n\' "$names" | grep -Eic -- "$pattern" || true)"');
    expect(run).toContain('count="${count:-0}"');
    expect(run).toContain('[ "$count" -lt 1 ]');
    expect(run).toContain(
      "::error::draft release $RELEASE_TAG carries no app artifacts for $TRIPLE (pattern $pattern) — tauri-action may have skipped uploads; check the tauri-action logs, then re-run the release",
    );
    expect(check.env?.RELEASE_TAG).toBe("${{ needs.gate.outputs.release_tag }}");
    expect(check.env?.TRIPLE).toBe("${{ matrix.triple }}");
    expect(check.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(check.env?.GH_REPO).toBe("${{ github.repository }}");
  });

  it("runs the package gate on every row after the app-asset check", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const checkIdx = steps.findIndex((s) => stepRuns(s).includes('grep -Eic -- "$pattern"'));
    expect(checkIdx, "the anchoring app-asset check must exist — an ordering compare against -1 passes vacuously").toBeGreaterThanOrEqual(0);
    const gate = steps.find((s) => stepRuns(s).startsWith("node scripts/ci/package-gate.mjs"));
    expect(gate, "the package-gate step must exist").toBeDefined();
    expect(gate.name, "the step name must not claim the app binary is started").toBe(
      "package gate (engines present and usable from the built package)",
    );
    const gateIdx = steps.findIndex((s) => s === gate);
    expect(gateIdx, "the package gate must run after the app-asset check").toBeGreaterThan(checkIdx);
    expect(gate["if"], "the package gate must run on every matrix row").toBeUndefined();
    expect(gate["continue-on-error"], "the package gate is a release blocker — a soft failure would ship a broken package").toBeUndefined();
    expect(gate.env?.TRIPLE, "the triple must reach the step via env, not raw ${{ }} in run").toBe("${{ matrix.triple }}");
    expect(gate.shell).toBe("bash");
  });

  it("has no finalize job (the gate pre-creates the draft, matrix rows upload into it)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(Object.keys(jobsOf(doc))).toContain("release");
    expect(Object.keys(jobsOf(doc))).not.toContain("finalize");
  });

  it("uploads sbom + third-party notices from the arm64 mac job after tauri-action and verifies the upload took", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).release);
    const sbomIdx = steps.findIndex((s) => stepRuns(s).includes("yarn sbom"));
    const actionIdx = steps.findIndex((s) => stepUses(s).startsWith("tauri-apps/tauri-action@"));
    expect(sbomIdx, "sbom step must exist").toBeGreaterThanOrEqual(0);
    expect(sbomIdx).toBeGreaterThan(actionIdx);

    const sbom = steps[sbomIdx];
    expect(sbom["if"]).toContain("aarch64-apple-darwin");
    expect(sbom.run).toContain('gh release upload "$RELEASE_TAG" sbom.json THIRD_PARTY_LICENSES.md --clobber');
    expect(
      sbom.env?.RELEASE_TAG,
      "RELEASE_TAG must come from the gate outputs, not from this row's own env",
    ).toBe("${{ needs.gate.outputs.release_tag }}");
    expect(sbom.run).toContain('gh release view "$RELEASE_TAG" --json assets --jq');
    expect(sbom.run, "post-upload verify counts the two expected asset names by default").toContain(
      '[ "$n" -ne 2 ]',
    );
    expect(sbom.run).toContain(
      "::error::release $RELEASE_TAG carries $n/2 of the expected sbom assets (sbom.json, THIRD_PARTY_LICENSES.md)",
    );
    expect(sbom.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(sbom.env?.GH_REPO).toBe("${{ github.repository }}");
  });

  it("prerelease flag is applied at gate draft creation from the helper-provided env", async () => {    const { doc, text } = await loadWorkflow(FILENAME);
    const draft = stepsOf(jobsOf(doc).gate).find((s) => stepRuns(s).includes('gh release create "$RELEASE_TAG"'));
    expect(draft.run).toContain('[ "$PRERELEASE" = "true" ]');
    expect(draft.run).toContain("draft_flags+=(--prerelease)");
    expect(
      text.includes("${{ env.PRERELEASE }}"),
      "PRERELEASE must flow from the helper's GITHUB_ENV, not be rewritten per job",
    ).toBe(false);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        if (stepRuns(step)) {
          expect(step.run, "no step may decide prerelease itself — the gate draft is authoritative").not.toContain(
            "-rc",
          );
          for (const [envName] of Object.entries(step.env ?? {})) {
            expect(envName).not.toBe("PRERELEASE");
          }
        }
      }
    }
  });
});

describe("ci.yml PR gate workflow", () => {
  const FILENAME = "ci.yml";

  it("has a self-check status of current", () => {
    expect(EXPECTED_WORKFLOWS.find((wf) => wf.file === FILENAME)?.status).toBe("current");
  });

  it("triggers on pull_request (all branches) and push to main only", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const triggers = doc.on ?? {};
    expect(Object.keys(triggers).sort()).toEqual(["pull_request", "push"]);
    expect(triggers.pull_request ?? null, "pull_request carries no branch filter — every PR is gated").toBeNull();
    expect(triggers.push.branches).toEqual(["main"]);
    expect(triggers.push.tags).toBeUndefined();
  });

  it("cancels superseded runs per ref", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.concurrency.group).toBe("gate-${{ github.ref }}");
    expect(doc.concurrency["cancel-in-progress"]).toBe(true);
  });

  it("scopes permissions to contents: read at workflow and every job", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    expect(doc.permissions).toEqual({ contents: "read" });
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      expect(job.permissions, `job ${jobName} must hold only contents: read (unfreeze downloads a repo release asset)`).toEqual({
        contents: "read",
      });
    }
  });

  it("has exactly the two-row gate matrix — mac + windows freeze on macos-15 / windows-2025 (Ruling: no ubuntu, no win-arm)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const gate = jobsOf(doc).gate;
    expect(gate, "gate job must exist").toBeDefined();
    expect(gate["runs-on"]).toBe("${{ matrix.os }}");
    expect(gate["timeout-minutes"], "the 6-hour hang class is structurally dead").toBe(60);
    expect(gate.strategy["fail-fast"]).toBe(false);
    expect(gate.strategy.matrix.include).toEqual(GATE_MATRIX_ROWS);

    const matrixText = JSON.stringify(gate.strategy.matrix);
    expect(matrixText, "both gate rows are consumers now — no stage field remains").not.toContain("stage");
    expect(matrixText, "linux rows never had a stageable linux engine (Ruling)").not.toContain("ubuntu");
    expect(matrixText, "win-arm coverage is tag-only (Ruling)").not.toContain("windows-11-arm");
    expect(matrixText, "win-arm coverage is tag-only (Ruling)").not.toContain("aarch64-pc-windows-msvc");
    expect(matrixText, "the stage field collapsed away — every row unfreezes (Ruling)").not.toContain("stage");
    for (const row of GATE_MATRIX_ROWS) {
      expect(row.os, "gate runner labels must never drift to -latest").not.toMatch(/-latest$/);
    }
  });

  it("every row restores frozen engines from the newest sidecars release with a read token (Ruling: no manifest staging in the gate)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const unfreezeStep = stepsOf(jobsOf(doc).gate).find((s) =>
      stepRuns(s).includes("bash scripts/ci/unfreeze.sh --triple"),
    );
    expect(unfreezeStep, "unfreeze step must exist for every gate row").toBeDefined();
    expect(unfreezeStep.run.trim(), "unfreeze run must stay exactly the unfreeze invocation (no || true, no pipes)").toBe(
      'bash scripts/ci/unfreeze.sh --triple "$TRIPLE"',
    );
    expect(unfreezeStep["if"], "both gate rows unfreeze from sidecars-* — no stage conditioning remains").toBeUndefined();
    expect(unfreezeStep.env?.TRIPLE).toBe("${{ matrix.triple }}");
    expect(unfreezeStep.env?.GH_TOKEN).toBe("${{ github.token }}");
    expect(unfreezeStep.env?.GH_REPO).toBe("${{ github.repository }}");

    const syncStep = stepsOf(jobsOf(doc).gate).find((s) =>
      stepRuns(s).includes("scripts/binaries/sync-macos-frameworks.mjs"),
    );
    expect(syncStep, "mac bundles carry tauri.conf.json dylib entries that the restored arm tree must re-derive — the sync must run on consumer rows or the clean-room audit dies with 'tauri.conf.json lists missing dylib' (CI run 39011908076)").toBeDefined();
    expect(syncStep["if"]).toBe("runner.os == 'macOS'");
    expect(syncStep.env?.TRIPLE, "the target must flow through env, not interpolated into the run string").toBe("${{ matrix.triple }}");
    expect(syncStep.run).toContain('node scripts/binaries/sync-macos-frameworks.mjs --target "$TRIPLE"');
    expect(syncStep.shell).toBe("bash");

    const unfreezeIdx2 = stepsOf(jobsOf(doc).gate).findIndex((s) => s === unfreezeStep);
    const syncIdx2 = stepsOf(jobsOf(doc).gate).findIndex((s) => s === syncStep);
    const macAuditIdx2 = stepsOf(jobsOf(doc).gate).findIndex(
      (s) => s["if"] === "runner.os == 'macOS'" && stepRuns(s).includes("yarn binaries:audit"),
    );
    expect(syncIdx2, "the sync must read the restored frameworks tree, so it runs after unfreeze").toBeGreaterThan(unfreezeIdx2);
    expect(macAuditIdx2, "the audit's conf cross-check must see the derived list, so it runs after the sync").toBeGreaterThan(syncIdx2);

    for (const s of stepsOf(jobsOf(doc).gate)) {
      if (stepRuns(s).includes("sync-macos-frameworks.mjs")) {
        expect(
          s["if"],
          "windows staging never creates src-tauri/frameworks — windows rows must skip the sync via the mac runner gate",
        ).toBe("runner.os == 'macOS'");
      }
    }

    expect(
      stepsOf(jobsOf(doc).gate).filter((s) => stepRuns(s).includes("scripts/binaries/all.mjs")),
      "the gate never stages from the manifest (Ruling)",
    ).toEqual([]);
  });

  it("audits the restored engines on every row — only mac rows allow the inactive-brew drift gate", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const steps = stepsOf(jobsOf(doc).gate);
    const audits = steps.filter((s) => stepRuns(s).includes("yarn binaries:audit"));
    expect(audits, "one audit step per runner os").toHaveLength(2);

    const mac = audits.find((s) => s["if"] === "runner.os == 'macOS'");
    const win = audits.find((s) => s["if"] === "runner.os == 'Windows'");
    expect(mac, "mac audit step must exist").toBeDefined();
    expect(win, "windows audit step must exist").toBeDefined();
    expect(
      mac.env?.BINARIES_ALLOW_INACTIVE_BREW,
      "runner brew has no vips/ghostscript kegs — the inactive allowance is what keeps the audit from dying on brew-present/formula-missing hosts",
    ).toBe("1");
    expect(
      win.env?.BINARIES_ALLOW_INACTIVE_BREW,
      "windows rows must not carry the mac brew allowance",
    ).toBeUndefined();
  });

  it("is a pure consumer: no brew, no all.mjs staging, no vendor fetch anywhere in the workflow", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        const runText = stepRuns(step);
        if (!runText) continue;
        expect(runText, `job ${jobName} step "${step.name}": consumers never invoke brew`).not.toMatch(/brew/i);
        expect(runText, `job ${jobName} step "${step.name}": consumers never stage via all.mjs`).not.toContain("all.mjs");
        expect(runText, `job ${jobName} step "${step.name}": consumers never vendor engines locally`).not.toMatch(/vendor/i);
      }
    }
  });

  it("runs the gate command verbatim as a single run string after engine restoration", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const gate = jobsOf(doc).gate;
    const steps = stepsOf(gate);
    const gateIdx = runIndexesByText(gate, GATE_COMMAND)[0];
    expect(gateIdx, "gate command must appear verbatim").toBeGreaterThanOrEqual(0);
    expect(steps[gateIdx].run.trim(), "the gate chain must stay one run string (workflows.test.mjs string check)").toBe(
      GATE_COMMAND,
    );

    const checkoutIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/checkout@"));
    const setupNodeIdx = steps.findIndex((s) => stepUses(s).startsWith("actions/setup-node@"));
    const installIdx = runIndexesByText(gate, "yarn install --frozen-lockfile")[0];
    const toolchainIdx = steps.findIndex((s) => stepUses(s).startsWith("dtolnay/rust-toolchain@"));
    const cacheIdx = steps.findIndex((s) => /swatinem\/rust-cache@/i.test(stepUses(s)));
    const unfreezeIdx = steps.findIndex((s) => stepRuns(s).includes("bash scripts/ci/unfreeze.sh --triple"));
    const macAuditIdx = steps.findIndex((s) => s["if"] === "runner.os == 'macOS'" && stepRuns(s).includes("yarn binaries:audit"));
    const winAuditIdx = steps.findIndex((s) => s["if"] === "runner.os == 'Windows'" && stepRuns(s).includes("yarn binaries:audit"));

    const observed = [checkoutIdx, setupNodeIdx, installIdx, toolchainIdx, cacheIdx, unfreezeIdx, macAuditIdx, winAuditIdx, gateIdx];
    for (const [i, idx] of observed.entries()) {
      const label = ["checkout", "setup-node", "install", "toolchain", "rust-cache", "unfreeze", "mac audit", "windows audit", "gate"][i];
      expect(idx, `step group "${label}" is present`).toBeGreaterThanOrEqual(0);
      if (i > 0) {
        expect(idx, `step group "${label}" is ordered`).toBeGreaterThan(observed[i - 1]);
      }
    }
  });

  it("builds no app bundles and uploads nothing", async () => {
    const { doc, text } = await loadWorkflow(FILENAME);
    expect(text, "the PR gate compiles and tests but never ships a bundle").not.toContain("tauri-action");
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        expect(stepRuns(step), `gate job ${jobName} must not build the app bundle`).not.toMatch(/tauri build/);
        const uses = stepUses(step);
        expect(uses.startsWith("actions/upload-artifact@"), `job ${jobName}: gate uploads no artifacts`).toBe(false);
        expect(uses.startsWith("actions/download-artifact@"), `job ${jobName}: gate downloads no artifacts`).toBe(false);
      }
    }
  });

  it("speeds repeat PR cargo builds with rust-cache keyed per os over the src-tauri workspace", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const cache = stepsOf(jobsOf(doc).gate).find((s) => /swatinem\/rust-cache@/i.test(stepUses(s)));
    expect(cache, "rust-cache step must exist").toBeDefined();
    expect(cache.with.key).toContain("${{ matrix.os }}");
    expect(cache.with.workspaces).toBe("src-tauri");
  });

  it("setup-node pins node 22 with the yarn cache", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    const setupNode = stepsOf(jobsOf(doc).gate).find((s) => stepUses(s).startsWith("actions/setup-node@"));
    expect(setupNode.with["node-version"]).toBe(22);
    expect(setupNode.with.cache).toBe("yarn");
  });

  it("runs every job under an explicit timeout (invariant: no unbounded jobs)", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      expect(
        typeof job["timeout-minutes"] === "number" && job["timeout-minutes"] > 0,
        `job ${jobName} must carry a positive timeout-minutes`,
      ).toBe(true);
    }
  });

  it("no run: step interpolates raw ${{ }} templates — context flows through step env", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        const runText = stepRuns(step);
        if (!runText) continue;
        expect(
          runText.includes("${{"),
          `job ${jobName} step "${step.name}": run scripts must not embed raw templates — pass context through step env`,
        ).toBe(false);
      }
    }
  });

  it("every run: step pins shell: bash", async () => {
    const { doc } = await loadWorkflow(FILENAME);
    for (const [jobName, job] of Object.entries(jobsOf(doc))) {
      for (const step of stepsOf(job)) {
        if (!stepRuns(step)) continue;
        expect(step.shell, `job ${jobName} step "${step.name}": run steps must set shell: bash`).toBe("bash");
      }
    }
  });
});

describe("tauri.conf.json signing", () => {
  it("bundle.macOS carries the official ad-hoc signing identity '-'", async () => {
    const conf = JSON.parse(
      await readFile(join(HERE, "..", "..", "src-tauri", "tauri.conf.json"), "utf8"),
    );
    expect(conf.bundle.macOS.signingIdentity).toBe("-");
    expect(
      conf.bundle.macOS.frameworks?.length,
      "the dylib bundle list stays intact next to the new key",
    ).toBeGreaterThan(0);
  });
});
