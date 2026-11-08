import { readFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = join(HERE, "..", "..");
const DEFAULT_CONF = join(REPO_ROOT, "src-tauri", "tauri.conf.json");

export const PLACEHOLDER_PUBKEY = "REPLACE_WITH_UPDATER_PUBLIC_KEY";
export const EXPECTED_ENDPOINT =
  "https://github.com/initgrep-apps/yard/releases/latest/download/latest.json";

export function verifyUpdaterConfig(
  config,
  { requireSecret = false, env = process.env } = {},
) {
  if (config?.bundle?.createUpdaterArtifacts !== true) {
    throw new Error(
      "bundle.createUpdaterArtifacts must be true in src-tauri/tauri.conf.json — the release bundle must ship updater artifacts",
    );
  }

  const updater = config?.plugins?.updater;
  const pubkey = updater?.pubkey;
  if (typeof pubkey !== "string" || pubkey.trim() === "") {
    throw new Error(
      "plugins.updater.pubkey is missing — set the updater signing public key in src-tauri/tauri.conf.json",
    );
  }
  if (pubkey === PLACEHOLDER_PUBKEY) {
    throw new Error(
      `plugins.updater.pubkey is still the placeholder public key ('${PLACEHOLDER_PUBKEY}') — replace it with the real signing public key in src-tauri/tauri.conf.json`,
    );
  }

  const endpoints = Array.isArray(updater?.endpoints)
    ? updater.endpoints
    : [];
  if (!endpoints.includes(EXPECTED_ENDPOINT)) {
    throw new Error(
      `plugins.updater.endpoints must include '${EXPECTED_ENDPOINT}' — point the updater at the release manifest in src-tauri/tauri.conf.json`,
    );
  }

  if (updater?.requireSignedVersion !== true) {
    throw new Error(
      "plugins.updater.requireSignedVersion must be true in src-tauri/tauri.conf.json — the updater must refuse unsigned versions",
    );
  }

  if (requireSecret) {
    const secret = env?.TAURI_SIGNING_PRIVATE_KEY;
    if (typeof secret !== "string" || secret.trim() === "") {
      throw new Error(
        "TAURI_SIGNING_PRIVATE_KEY is not set — add the updater signing key as a repository secret before releasing",
      );
    }
  }
}

export function parseArgs(argv) {
  let configPath = DEFAULT_CONF;
  let requireSecret = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--config") {
      const value = argv[index + 1];
      if (!value) {
        throw new Error("--config requires a path to a tauri.conf.json file");
      }
      configPath = value;
      index += 1;
    } else if (arg === "--require-secret") {
      requireSecret = true;
    } else {
      throw new Error(
        `unknown argument '${arg}' — usage: node scripts/ci/updater-preflight.mjs [--config <path>] [--require-secret]`,
      );
    }
  }
  return { configPath, requireSecret };
}

export function main(argv = [], { env = process.env } = {}) {
  const { configPath, requireSecret } = parseArgs(argv);
  let config;
  try {
    config = JSON.parse(readFileSync(configPath, "utf8"));
  } catch (error) {
    throw new Error(
      `cannot read the updater config at '${configPath}' — ${error.message}`,
    );
  }
  verifyUpdaterConfig(config, { requireSecret, env });
  console.log("updater preflight: OK");
}

if (isMain(import.meta.url)) {
  try {
    main(process.argv.slice(2), { env: process.env });
  } catch (error) {
    console.log(`::error::updater-preflight: ${error.message}`);
    process.exit(1);
  }
}

function isMain(importMetaUrl) {
  return Boolean(process.argv[1]) && import.meta.url.endsWith(basename(process.argv[1]));
}
