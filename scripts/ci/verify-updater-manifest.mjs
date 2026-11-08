import { readFileSync } from "node:fs";
import { basename, join } from "node:path";

export const REQUIRED_PLATFORMS = [
  "darwin-aarch64",
  "darwin-x86_64",
  "windows-aarch64",
  "windows-x86_64",
  "windows-aarch64-nsis",
  "windows-aarch64-msi",
  "windows-x86_64-nsis",
  "windows-x86_64-msi",
];

const USAGE =
  "usage: node scripts/ci/verify-updater-manifest.mjs --version <version> --manifest <latest.json> --assets <assets.json> --sigs-dir <dir>";
const ASSET_ID_PATTERN = /\/releases\/assets\/(\d+)/;
const DOWNLOAD_NAME_PATTERN = /\/releases\/download\/[^/]+\/(.+)$/;
const ASSET_URL_FIELDS = ["apiUrl", "downloadUrl", "browser_download_url", "url"];

export function verifyUpdaterManifest({
  manifest,
  assets,
  sigsDir,
  expectedVersion,
}) {
  if (typeof expectedVersion !== "string" || expectedVersion.trim() === "") {
    throw new Error("expectedVersion is required");
  }
  const version = expectedVersion.trim();
  if (manifest?.version !== version) {
    throw new Error(
      `latest.json version '${manifest?.version ?? ""}' does not match the expected version '${version}'`,
    );
  }

  const platforms = manifest?.platforms ?? {};
  const assetList = normalizeAssets(assets);

  for (const platform of REQUIRED_PLATFORMS) {
    const entry = platforms[platform];
    if (!entry || typeof entry !== "object") {
      throw new Error(
        `latest.json is missing the '${platform}' platform entry`,
      );
    }

    const signature =
      typeof entry.signature === "string" ? entry.signature.trim() : "";
    if (signature === "") {
      throw new Error(`latest.json entry '${platform}' has an empty signature`);
    }

    const url = typeof entry.url === "string" ? entry.url.trim() : "";
    if (url === "") {
      throw new Error(`latest.json entry '${platform}' has no url`);
    }

    const asset = findAsset(url, assetList);
    if (!asset) {
      throw new Error(
        `latest.json entry '${platform}' points at '${url}' which is not an asset in the release`,
      );
    }
    if (typeof asset.name !== "string" || asset.name === "") {
      throw new Error(`the release asset for '${platform}' has no name`);
    }

    const sigPath = join(sigsDir, `${asset.name}.sig`);
    let sig;
    try {
      sig = readFileSync(sigPath, "utf8");
    } catch {
      throw new Error(
        `missing signature file '${sigPath}' for platform '${platform}'`,
      );
    }
    if (sig.trim() !== signature) {
      throw new Error(
        `the signature for '${asset.name}' does not match the uploaded '.sig' file`,
      );
    }
  }
}

export function parseArgs(argv) {
  const flags = {
    "--version": "version",
    "--manifest": "manifestPath",
    "--assets": "assetsPath",
    "--sigs-dir": "sigsDir",
  };
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const key = flags[arg];
    if (!key) {
      throw new Error(`unknown argument '${arg}' — ${USAGE}`);
    }
    const value = argv[index + 1];
    if (!value) {
      throw new Error(`${arg} requires a value — ${USAGE}`);
    }
    options[key] = value;
    index += 1;
  }
  for (const [flag, key] of Object.entries(flags)) {
    if (!options[key]) {
      throw new Error(`${flag} is required — ${USAGE}`);
    }
  }
  return options;
}

export function main(argv = []) {
  const { version, manifestPath, assetsPath, sigsDir } = parseArgs(argv);
  verifyUpdaterManifest({
    manifest: readJson(manifestPath),
    assets: readJson(assetsPath),
    sigsDir,
    expectedVersion: version,
  });
  console.log("updater manifest: OK");
}

function normalizeAssets(assets) {
  if (Array.isArray(assets)) {
    return assets;
  }
  if (Array.isArray(assets?.assets)) {
    return assets.assets;
  }
  return [];
}

function findAsset(url, assets) {
  const assetId = ASSET_ID_PATTERN.exec(url)?.[1];
  const downloadName = decodeURIComponent(
    DOWNLOAD_NAME_PATTERN.exec(url)?.[1] ?? "",
  );
  return assets.find((asset) => {
    if (assetUrls(asset).includes(url)) {
      return true;
    }
    if (assetId !== undefined && String(asset.id ?? "") === assetId) {
      return true;
    }
    return downloadName !== "" && asset.name === downloadName;
  });
}

function assetUrls(asset) {
  return ASSET_URL_FIELDS.map((field) => asset?.[field])
    .filter((value) => typeof value === "string" && value.trim() !== "")
    .map((value) => value.trim());
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`cannot read '${path}' — ${error.message}`);
  }
}

if (isMain(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.log(`::error::verify-updater-manifest: ${error.message}`);
    process.exitCode = 1;
  }
}

function isMain(importMetaUrl) {
  return Boolean(process.argv[1]) && import.meta.url.endsWith(basename(process.argv[1]));
}
