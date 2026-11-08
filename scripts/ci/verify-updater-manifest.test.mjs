// @vitest-environment node
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  REQUIRED_PLATFORMS,
  verifyUpdaterManifest,
} from "./verify-updater-manifest.mjs";

const SCRIPT = join(import.meta.dirname, "verify-updater-manifest.mjs");
const VERSION = "1.2.3";
const REPO = "initgrep-apps/yard";

const ASSET_NAMES = {
  "darwin-aarch64": `yard_${VERSION}_aarch64.app.tar.gz`,
  "darwin-x86_64": `yard_${VERSION}_x64.app.tar.gz`,
  "windows-aarch64": `yard_${VERSION}_arm64-setup.exe`,
  "windows-x86_64": `yard_${VERSION}_x64-setup.exe`,
  "windows-aarch64-nsis": `yard_${VERSION}_arm64-setup.exe`,
  "windows-aarch64-msi": `yard_${VERSION}_arm64_en-US.msi`,
  "windows-x86_64-nsis": `yard_${VERSION}_x64-setup.exe`,
  "windows-x86_64-msi": `yard_${VERSION}_x64_en-US.msi`,
};

const UNIQUE_ASSET_NAMES = [...new Set(Object.values(ASSET_NAMES))];

function buildAssets() {
  return UNIQUE_ASSET_NAMES.map((name, index) => {
    const id = String(index + 1);
    return {
      id,
      name,
      apiUrl: `https://api.github.com/repos/${REPO}/releases/assets/${id}`,
      downloadUrl: `https://github.com/${REPO}/releases/download/v${VERSION}/${name}`,
      url: `https://github.com/${REPO}/releases/assets/${id}`,
    };
  });
}

function buildManifest(assets) {
  const byName = new Map(assets.map((asset) => [asset.name, asset]));
  const platforms = {};
  for (const platform of REQUIRED_PLATFORMS) {
    const asset = byName.get(ASSET_NAMES[platform]);
    platforms[platform] = {
      signature: `sig-${asset.name}`,
      url: asset.downloadUrl,
    };
  }
  return { version: VERSION, platforms };
}

function buildFixture({ mutateManifest } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "verify-updater-manifest-"));
  const sigsDir = join(dir, "sigs");
  mkdirSync(sigsDir);
  const assets = buildAssets();
  for (const asset of assets) {
    writeFileSync(join(sigsDir, `${asset.name}.sig`), `sig-${asset.name}\n`);
  }
  const manifest = buildManifest(assets);
  const finalManifest = structuredClone(manifest);
  mutateManifest?.(finalManifest, assets);
  const manifestPath = join(dir, "latest.json");
  const assetsPath = join(dir, "assets.json");
  writeFileSync(manifestPath, JSON.stringify(finalManifest, null, 2));
  writeFileSync(assetsPath, JSON.stringify({ assets }, null, 2));
  return { dir, sigsDir, manifestPath, assetsPath, assets, manifest: finalManifest };
}

describe("verifyUpdaterManifest", () => {
  it("accepts a complete manifest whose signatures match the sig files", () => {
    const fixture = buildFixture();
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).not.toThrow();
  });

  it("rejects a version mismatch", () => {
    const fixture = buildFixture();
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: "9.9.9",
      }),
    ).toThrow(/does not match the expected version/);
  });

  it("rejects a missing platform key", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest) => {
        delete manifest.platforms["windows-x86_64-msi"];
      },
    });
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).toThrow(/windows-x86_64-msi/);
  });

  it("rejects an empty signature", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest) => {
        manifest.platforms["darwin-aarch64"].signature = "";
      },
    });
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).toThrow(/empty signature/);
  });

  it("rejects a URL that does not match an asset", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest) => {
        manifest.platforms["darwin-x86_64"].url =
          `https://github.com/${REPO}/releases/download/v${VERSION}/missing.app.tar.gz`;
      },
    });
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).toThrow(/is not an asset in the release/);
  });

  it("rejects a signature that differs from the sig file", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest) => {
        manifest.platforms["darwin-aarch64"].signature = "sig-not-the-file";
      },
    });
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).toThrow(/does not match/);
  });

  it("rejects a missing sig file", () => {
    const fixture = buildFixture();
    rmSync(join(fixture.sigsDir, `${ASSET_NAMES["darwin-aarch64"]}.sig`));
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).toThrow(/missing signature file/);
  });

  it("matches an api asset url", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest, assets) => {
        const asset = assets.find(
          (candidate) => candidate.name === ASSET_NAMES["darwin-aarch64"],
        );
        manifest.platforms["darwin-aarch64"].url = asset.apiUrl;
      },
    });
    expect(() =>
      verifyUpdaterManifest({
        manifest: fixture.manifest,
        assets: fixture.assets,
        sigsDir: fixture.sigsDir,
        expectedVersion: VERSION,
      }),
    ).not.toThrow();
  });
});

describe("verify-updater-manifest CLI", () => {
  it("exits 0 and prints OK for a valid manifest", () => {
    const fixture = buildFixture();
    const result = spawnSync(
      process.execPath,
      [
        SCRIPT,
        "--version",
        VERSION,
        "--manifest",
        fixture.manifestPath,
        "--assets",
        fixture.assetsPath,
        "--sigs-dir",
        fixture.sigsDir,
      ],
      { encoding: "utf8", timeout: 30_000 },
    );

    expect(result.status, result.stderr).toBe(0);
    expect(result.stdout).toContain("updater manifest: OK");
  });

  it("exits 1 and prints ::error:: for a missing platform", () => {
    const fixture = buildFixture({
      mutateManifest: (manifest) => {
        delete manifest.platforms["windows-x86_64-msi"];
      },
    });
    const result = spawnSync(
      process.execPath,
      [
        SCRIPT,
        "--version",
        VERSION,
        "--manifest",
        fixture.manifestPath,
        "--assets",
        fixture.assetsPath,
        "--sigs-dir",
        fixture.sigsDir,
      ],
      { encoding: "utf8", timeout: 30_000 },
    );

    expect(result.status, result.stderr).toBe(1);
    expect(result.stdout).toContain("::error::");
  });
});
