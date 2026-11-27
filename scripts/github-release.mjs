#!/usr/bin/env node
// GitHub Release steps for .github/workflows/release.yml.
//
//   node scripts/github-release.mjs create  --tag v0.2.0 --target <sha> [--prerelease true|false]
//   node scripts/github-release.mjs publish --release-id <id>
//
// `create` makes (or reuses) one draft release populated with the version's
// section from CHANGELOG.md so every build job uploads to it.
// `publish` runs after all builds succeed: it cryptographically verifies every
// `.sig` against the public key in tauri.conf.json, uploads `SHA256SUMS.txt`
// and a single `latest.json` covering every platform, then publishes the
// release so `releases/latest/download/latest.json` points at it.
//
// Requires GITHUB_TOKEN and GITHUB_REPOSITORY (set automatically in Actions).

import { createHash, createPublicKey, verify as cryptoVerify } from "node:crypto";
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readChangelogNotes } from "./release.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tauriConfigPath = join(root, "apps/desktop/src-tauri/tauri.conf.json");

const API = process.env.GITHUB_API_URL ?? "https://api.github.com";
const SERVER = process.env.GITHUB_SERVER_URL ?? "https://github.com";

const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

/**
 * Updater targets served by each signed bundle. The app looks up
 * `darwin-universal` on macOS (see lib.rs); the per-arch keys keep the manifest
 * usable if that override is ever removed. Windows NSIS tries
 * `windows-x86_64-nsis` before `windows-x86_64`.
 */
export const PLATFORM_BUNDLES = [
  { suffix: "-setup.exe", targets: ["windows-x86_64", "windows-x86_64-nsis"] },
  { suffix: ".app.tar.gz", targets: ["darwin-universal", "darwin-aarch64", "darwin-x86_64"] },
];

/**
 * Decodes a Tauri minisign public key (base64-encoded 2-line minisign pubkey)
 * into its 8-byte key ID and a Node.js Ed25519 KeyObject.
 */
export function parseMinisignPublicKey(encodedPubkey) {
  const decodedText = Buffer.from(encodedPubkey.trim(), "base64").toString("utf8");
  const lines = decodedText.trim().split(/\r?\n/);
  if (lines.length < 2) {
    throw new Error("Invalid minisign public key: expected 2 lines after base64 decode");
  }
  const raw = Buffer.from(lines[1].trim(), "base64");
  if (raw.length !== 42) {
    throw new Error(`Invalid minisign public key payload length: expected 42, got ${raw.length}`);
  }
  const alg = raw.toString("ascii", 0, 2);
  if (alg !== "Ed" && alg !== "ED") {
    throw new Error(`Unsupported minisign public key algorithm: ${alg}`);
  }
  const keyId = raw.subarray(2, 10);
  const publicKeyBytes = raw.subarray(10, 42);
  const keyObject = createPublicKey({
    key: Buffer.concat([ED25519_SPKI_PREFIX, publicKeyBytes]),
    format: "der",
    type: "spki",
  });
  return { keyId, keyObject, untrustedComment: lines[0] };
}

/**
 * Decodes a Tauri `.sig` file (base64-encoded 4-line minisign signature).
 */
export function parseMinisignSignature(encodedSignature) {
  const decodedText = Buffer.from(encodedSignature.trim(), "base64").toString("utf8");
  const lines = decodedText.trim().split(/\r?\n/);
  if (lines.length < 4) {
    throw new Error("Invalid minisign signature: expected 4 lines after base64 decode");
  }
  const sigBlock = Buffer.from(lines[1].trim(), "base64");
  if (sigBlock.length !== 74) {
    throw new Error(`Invalid minisign signature block length: expected 74, got ${sigBlock.length}`);
  }
  const alg = sigBlock.toString("ascii", 0, 2);
  if (alg !== "Ed" && alg !== "ED") {
    throw new Error(`Unsupported minisign signature algorithm: ${alg}`);
  }
  const trustedPrefix = "trusted comment: ";
  if (!lines[2].startsWith(trustedPrefix)) {
    throw new Error("Invalid minisign signature: missing trusted comment");
  }
  const globalSignature = Buffer.from(lines[3].trim(), "base64");
  if (globalSignature.length !== 64) {
    throw new Error(`Invalid minisign global signature length: expected 64, got ${globalSignature.length}`);
  }
  return {
    prehashed: alg === "ED",
    keyId: sigBlock.subarray(2, 10),
    signature: sigBlock.subarray(10, 74),
    trustedComment: lines[2].slice(trustedPrefix.length),
    globalSignature,
  };
}

/**
 * Verifies a Tauri updater signature against artifact bytes and the app's
 * embedded minisign public key (matching tauri-plugin-updater's verification).
 */
export function verifyMinisignSignature({ data, signature, pubkey, expectedVersion }) {
  const pk = parseMinisignPublicKey(pubkey);
  const sig = parseMinisignSignature(signature);
  if (!sig.keyId.equals(pk.keyId)) {
    throw new Error(
      `Signature key ID ${sig.keyId.toString("hex")} does not match tauri.conf.json public key ${pk.keyId.toString("hex")}`,
    );
  }
  const bytes = Buffer.isBuffer(data) ? data : Buffer.from(data);
  const payload = sig.prehashed ? createHash("blake2b512").update(bytes).digest() : bytes;
  if (!cryptoVerify(null, payload, pk.keyObject, sig.signature)) {
    throw new Error("Artifact Ed25519 signature verification failed");
  }
  const globalPayload = Buffer.concat([sig.signature, Buffer.from(sig.trustedComment, "utf8")]);
  if (!cryptoVerify(null, globalPayload, pk.keyObject, sig.globalSignature)) {
    throw new Error("Trusted comment global signature verification failed");
  }
  if (expectedVersion) {
    const signedVersion = sig.trustedComment
      .split("\t")
      .find((field) => field.startsWith("version:"))
      ?.slice("version:".length);
    if (signedVersion) {
      const normalizedSigned = signedVersion.replace(/^v/, "");
      const normalizedExpected = expectedVersion.replace(/^v/, "");
      if (normalizedSigned !== normalizedExpected) {
        throw new Error(`Signature version ${normalizedSigned} does not match release version ${normalizedExpected}`);
      }
    }
  }
  return { keyId: sig.keyId.toString("hex"), prehashed: sig.prehashed, trustedComment: sig.trustedComment };
}

/**
 * Builds a deterministic `sha256sum`-compatible checksum manifest.
 * `entries` is an array of `{ name, data }`.
 */
export function buildChecksums(entries) {
  return `${[...entries]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(({ name, data }) => {
      const digest = createHash("sha256").update(Buffer.isBuffer(data) ? data : Buffer.from(data)).digest("hex");
      return `${digest}  ${name}`;
    })
    .join("\n")}\n`;
}

/**
 * Builds the static updater manifest from a release's asset list.
 * `signatures` maps a `.sig` asset name to its text content.
 * When `pubkey` and `artifacts` are provided, every signature is verified
 * cryptographically before inclusion.
 */
export function buildManifest({ repository, tag, notes, assets, signatures, pubkey, artifacts, pubDate = new Date() }) {
  const names = new Set(assets.map((asset) => asset.name));
  const version = tag.replace(/^v/, "");
  const platforms = {};
  const missing = [];
  for (const bundle of PLATFORM_BUNDLES) {
    const sigs = assets.filter((asset) => asset.name.endsWith(`${bundle.suffix}.sig`));
    if (sigs.length === 0) {
      missing.push(bundle.suffix);
      continue;
    }
    if (sigs.length > 1) {
      throw new Error(`Expected one ${bundle.suffix} bundle, found: ${sigs.map((asset) => asset.name).join(", ")}`);
    }
    const sigName = sigs[0].name;
    const bundleName = sigName.slice(0, -".sig".length);
    if (!names.has(bundleName)) throw new Error(`${sigName} has no matching ${bundleName} asset`);
    const signature = signatures[sigName]?.trim();
    if (!signature) throw new Error(`${sigName} is empty`);
    if (pubkey) {
      const data = artifacts?.[bundleName];
      if (!data) throw new Error(`Missing downloaded artifact bytes for ${bundleName}`);
      try {
        verifyMinisignSignature({ data, signature, pubkey, expectedVersion: version });
      } catch (error) {
        throw new Error(`Signature verification failed for ${bundleName}: ${error.message}`);
      }
    }
    const url = `${SERVER}/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(bundleName)}`;
    for (const target of bundle.targets) platforms[target] = { signature, url };
  }
  if (missing.length > 0) {
    throw new Error(`Release is missing signed updater bundles: ${missing.join(", ")}. Did every build job succeed?`);
  }
  return {
    version,
    notes: notes?.trim() ?? "",
    pub_date: pubDate.toISOString(),
    platforms,
  };
}

function readUpdaterPubkey() {
  const config = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
  const pubkey = config?.plugins?.updater?.pubkey;
  if (!pubkey) throw new Error("apps/desktop/src-tauri/tauri.conf.json is missing plugins.updater.pubkey");
  return pubkey;
}

async function github(path, { method = "GET", body, headers = {}, raw = false, base = API } = {}) {
  const token = process.env.GITHUB_TOKEN;
  if (!token) throw new Error("GITHUB_TOKEN is not set");
  const response = await fetch(path.startsWith("http") ? path : `${base}${path}`, {
    method,
    headers: {
      authorization: `Bearer ${token}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
      ...(body && !raw ? { "content-type": "application/json" } : {}),
      ...headers,
    },
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`${method} ${path} failed: ${response.status} ${await response.text()}`);
  }
  if (response.status === 204) return null;
  return response.json();
}

/** Downloads a (possibly draft) release asset as a Buffer. */
async function downloadAssetBuffer(repo, assetId) {
  const path = `/repos/${repo}/releases/assets/${assetId}`;
  const response = await fetch(`${API}${path}`, {
    redirect: "manual",
    headers: {
      authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
      accept: "application/octet-stream",
      "x-github-api-version": "2022-11-28",
    },
  });
  if (response.ok) return Buffer.from(await response.arrayBuffer());
  const location = response.headers.get("location");
  if (response.status >= 300 && response.status < 400 && location) {
    // The redirect target is a pre-signed storage URL; it must not receive the token.
    const file = await fetch(location);
    if (file.ok) return Buffer.from(await file.arrayBuffer());
    throw new Error(`Downloading asset ${assetId} failed: ${file.status}`);
  }
  throw new Error(`GET ${path} failed: ${response.status} ${await response.text()}`);
}

function repository() {
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo) throw new Error("GITHUB_REPOSITORY is not set");
  return repo;
}

async function findRelease(repo, tag) {
  for (let page = 1; page <= 10; page += 1) {
    const releases = await github(`/repos/${repo}/releases?per_page=100&page=${page}`);
    const match = releases.find((release) => release.tag_name === tag);
    if (match || releases.length < 100) return match ?? null;
  }
  return null;
}

async function uploadOrReplaceAsset(repo, release, name, body, contentType) {
  const existing = release.assets.find((asset) => asset.name === name);
  if (existing) await github(`/repos/${repo}/releases/assets/${existing.id}`, { method: "DELETE" });
  const uploadUrl = release.upload_url.replace(/\{.*\}$/, "");
  await github(`${uploadUrl}?name=${encodeURIComponent(name)}`, {
    method: "POST",
    raw: true,
    body,
    headers: { "content-type": contentType },
  });
}

async function create({ tag, target, prerelease }) {
  if (!tag) throw new Error("--tag is required");
  const repo = repository();
  const version = tag.replace(/^v/, "");
  const notes = readChangelogNotes(version);
  const isPrerelease = prerelease === "true" || tag.includes("-");

  let release = await findRelease(repo, tag);
  if (release && !release.draft) {
    throw new Error(`${tag} is already published. Bump the version to release again.`);
  }
  if (release) {
    // A re-run starts from a clean draft so no stale bundle is published.
    console.log(`Reusing draft release ${release.id} for ${tag}; removing ${release.assets.length} old assets.`);
    for (const asset of release.assets) {
      await github(`/repos/${repo}/releases/assets/${asset.id}`, { method: "DELETE" });
    }
    release = await github(`/repos/${repo}/releases/${release.id}`, {
      method: "PATCH",
      body: {
        ...(target ? { target_commitish: target } : {}),
        name: `oh-my-voice-type ${tag}`,
        body: notes,
        prerelease: isPrerelease,
      },
    });
  } else {
    release = await github(`/repos/${repo}/releases`, {
      method: "POST",
      body: {
        tag_name: tag,
        target_commitish: target,
        name: `oh-my-voice-type ${tag}`,
        body: notes,
        draft: true,
        prerelease: isPrerelease,
      },
    });
    console.log(`Created draft release ${release.id} for ${tag}.`);
  }
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `release_id=${release.id}\n`);
  return release;
}

async function publish({ releaseId }) {
  if (!releaseId) throw new Error("--release-id is required");
  const repo = repository();
  const release = await github(`/repos/${repo}/releases/${releaseId}`);
  if (!release.draft) throw new Error(`Release ${release.tag_name} is already published`);

  const pubkey = readUpdaterPubkey();
  const signatures = {};
  const artifacts = {};
  const checksumEntries = [];

  for (const asset of release.assets) {
    if (asset.name === "latest.json" || asset.name === "SHA256SUMS.txt") continue;
    const buffer = await downloadAssetBuffer(repo, asset.id);
    if (asset.name.endsWith(".sig")) {
      signatures[asset.name] = buffer.toString("utf8");
    } else {
      artifacts[asset.name] = buffer;
      checksumEntries.push({ name: asset.name, data: buffer });
    }
  }

  const manifest = buildManifest({
    repository: repo,
    tag: release.tag_name,
    notes: release.body,
    assets: release.assets,
    signatures,
    pubkey,
    artifacts,
  });
  console.log(`Verified updater signatures and built latest.json for ${manifest.version}: ${Object.keys(manifest.platforms).join(", ")}`);

  const checksums = buildChecksums(checksumEntries);
  await uploadOrReplaceAsset(repo, release, "SHA256SUMS.txt", checksums, "text/plain; charset=utf-8");
  await uploadOrReplaceAsset(repo, release, "latest.json", `${JSON.stringify(manifest, null, 2)}\n`, "application/json");

  const published = await github(`/repos/${repo}/releases/${releaseId}`, {
    method: "PATCH",
    body: { draft: false, make_latest: release.prerelease ? "false" : "true" },
  });
  console.log(`Published ${published.html_url}`);
  if (!release.prerelease) await verifyLatest(repo, manifest.version);
}

/** Confirms the URL baked into the app now serves this version. */
async function verifyLatest(repo, version) {
  const url = `${SERVER}/${repo}/releases/latest/download/latest.json`;
  for (let attempt = 1; attempt <= 6; attempt += 1) {
    try {
      const response = await fetch(url, { headers: { "cache-control": "no-cache" } });
      if (response.ok) {
        const served = await response.json();
        if (served.version === version) {
          console.log(`Verified ${url} serves ${version}.`);
          return;
        }
        console.log(`Attempt ${attempt}: ${url} still serves ${served.version}`);
      } else {
        console.log(`Attempt ${attempt}: ${url} returned ${response.status}`);
      }
    } catch (error) {
      console.log(`Attempt ${attempt}: ${error.message}`);
    }
    await new Promise((done) => setTimeout(done, 10_000));
  }
  throw new Error(`${url} did not serve ${version} after publishing`);
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (!key.startsWith("--")) continue;
    const name = key.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    options[name] = argv[index + 1];
    index += 1;
  }
  return options;
}

async function main(argv) {
  const [command, ...rest] = argv;
  const options = parseArgs(rest);
  if (command === "create") return create(options);
  if (command === "publish") return publish(options);
  throw new Error(
    "Usage: node scripts/github-release.mjs create --tag <tag> [--target <sha>] [--prerelease true|false] | publish --release-id <id>",
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
