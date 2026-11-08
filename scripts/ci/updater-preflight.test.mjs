// @vitest-environment node
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EXPECTED_ENDPOINT,
  PLACEHOLDER_PUBKEY,
  verifyUpdaterConfig,
} from "./updater-preflight.mjs";

const SCRIPT = join(import.meta.dirname, "updater-preflight.mjs");

const valid = {
  bundle: { createUpdaterArtifacts: true },
  plugins: {
    updater: {
      pubkey: "A".repeat(56),
      endpoints: [EXPECTED_ENDPOINT],
      requireSignedVersion: true,
    },
  },
};

describe("verifyUpdaterConfig", () => {
  it("accepts a complete config", () => {
    expect(() => verifyUpdaterConfig(valid)).not.toThrow();
  });

  it("rejects updater artifacts being disabled", () => {
    const config = structuredClone(valid);
    config.bundle.createUpdaterArtifacts = false;
    expect(() => verifyUpdaterConfig(config)).toThrow(/createUpdaterArtifacts/);
  });

  it("rejects the placeholder public key", () => {
    const config = structuredClone(valid);
    config.plugins.updater.pubkey = PLACEHOLDER_PUBKEY;
    expect(() => verifyUpdaterConfig(config)).toThrow(/public key/i);
  });

  it("rejects a missing endpoint", () => {
    const config = structuredClone(valid);
    config.plugins.updater.endpoints = [];
    expect(() => verifyUpdaterConfig(config)).toThrow(/endpoint/i);
  });

  it("rejects a config without requireSignedVersion", () => {
    const config = structuredClone(valid);
    config.plugins.updater.requireSignedVersion = undefined;
    expect(() => verifyUpdaterConfig(config)).toThrow(/requireSignedVersion/);
  });

  it("requires the signing secret only when asked", () => {
    expect(() => verifyUpdaterConfig(valid, { env: {} })).not.toThrow();
    expect(() =>
      verifyUpdaterConfig(valid, { requireSecret: true, env: {} }),
    ).toThrow(/TAURI_SIGNING_PRIVATE_KEY/);
  });
});

describe("updater-preflight CLI", () => {
  it("exits 1 and prints ::error:: for a placeholder public key", () => {
    const dir = mkdtempSync(join(tmpdir(), "updater-preflight-"));
    const configPath = join(dir, "tauri.conf.json");
    const config = structuredClone(valid);
    config.plugins.updater.pubkey = PLACEHOLDER_PUBKEY;
    writeFileSync(configPath, JSON.stringify(config));

    const result = spawnSync(
      process.execPath,
      [SCRIPT, "--config", configPath],
      { encoding: "utf8", timeout: 30_000 },
    );

    expect(result.status, result.stderr).toBe(1);
    expect(result.stdout).toContain("::error::");
  });
});
