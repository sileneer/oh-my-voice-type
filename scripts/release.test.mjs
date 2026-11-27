// Run with: node --test scripts/release.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, sign as cryptoSign } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  buildChecksums,
  buildManifest,
  parseMinisignPublicKey,
  verifyMinisignSignature,
} from "./github-release.mjs";
import { extractChangelogSection, nextVersion } from "./release.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function makeMinisignPair() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const spki = publicKey.export({ format: "der", type: "spki" });
  const rawPub = spki.subarray(spki.length - 32);
  const keyId = randomBytes(8);
  const pubBlock = Buffer.concat([Buffer.from("Ed", "ascii"), keyId, rawPub]).toString("base64");
  const pubText = `untrusted comment: minisign public key: ${keyId.toString("hex").toUpperCase()}\n${pubBlock}\n`;
  const encodedPubkey = Buffer.from(pubText, "utf8").toString("base64");
  return { privateKey, keyId, encodedPubkey };
}

function makeMinisignSig({ privateKey, keyId, data, trustedComment = "timestamp:1700000000\tfile:test\tversion:0.2.0", prehashed = true }) {
  const bytes = Buffer.isBuffer(data) ? data : Buffer.from(data);
  const payload = prehashed ? createHash("blake2b512").update(bytes).digest() : bytes;
  const sig = cryptoSign(null, payload, privateKey);
  const globalSig = cryptoSign(null, Buffer.concat([sig, Buffer.from(trustedComment, "utf8")]), privateKey);
  const sigBlock = Buffer.concat([Buffer.from(prehashed ? "ED" : "Ed", "ascii"), keyId, sig]).toString("base64");
  const sigText = [
    "untrusted comment: signature from tauri secret key",
    sigBlock,
    `trusted comment: ${trustedComment}`,
    globalSig.toString("base64"),
  ].join("\n");
  return Buffer.from(`${sigText}\n`, "utf8").toString("base64");
}

test("nextVersion bumps semver parts or accepts an exact version", () => {
  assert.equal(nextVersion("0.1.0", "patch"), "0.1.1");
  assert.equal(nextVersion("0.1.9", "minor"), "0.2.0");
  assert.equal(nextVersion("1.4.2", "major"), "2.0.0");
  assert.equal(nextVersion("0.1.0", "0.3.0-beta.1"), "0.3.0-beta.1");
  assert.throws(() => nextVersion("0.1.0", "v0.2"), /not a semver/);
});

test("extractChangelogSection extracts notes for a version and rejects missing or empty entries", () => {
  const md = [
    "# Changelog",
    "",
    "## [Unreleased]",
    "",
    "## [0.2.0] - 2026-12-01",
    "",
    "### Added",
    "- New feature",
    "",
    "## [0.1.0]",
    "",
    "- Initial release",
    "",
    "## [0.0.1]",
    "",
  ].join("\n");

  assert.equal(extractChangelogSection(md, "0.2.0"), "### Added\n- New feature");
  assert.equal(extractChangelogSection(md, "v0.1.0"), "- Initial release");
  assert.throws(() => extractChangelogSection(md, "0.3.0"), /has no section for version 0\.3\.0/);
  assert.throws(() => extractChangelogSection(md, "0.0.1"), /section for version 0\.0\.1 is empty/);
});

test("tauri.conf.json updater public key decodes cleanly", () => {
  const tauriConf = JSON.parse(readFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
  const parsed = parseMinisignPublicKey(tauriConf.plugins.updater.pubkey);
  assert.equal(parsed.keyId.length, 8);
});

test("verifyMinisignSignature validates ED prehashed and Ed legacy signatures and rejects tampering", () => {
  const pair = makeMinisignPair();
  const other = makeMinisignPair();
  const payload = Buffer.from("installer-binary-content");

  const sigPrehashed = makeMinisignSig({ ...pair, data: payload, prehashed: true });
  const resultPrehashed = verifyMinisignSignature({
    data: payload,
    signature: sigPrehashed,
    pubkey: pair.encodedPubkey,
    expectedVersion: "0.2.0",
  });
  assert.equal(resultPrehashed.prehashed, true);

  const sigLegacy = makeMinisignSig({ ...pair, data: payload, prehashed: false });
  const resultLegacy = verifyMinisignSignature({
    data: payload,
    signature: sigLegacy,
    pubkey: pair.encodedPubkey,
    expectedVersion: "0.2.0",
  });
  assert.equal(resultLegacy.prehashed, false);

  assert.throws(
    () => verifyMinisignSignature({ data: Buffer.from("tampered"), signature: sigPrehashed, pubkey: pair.encodedPubkey, expectedVersion: "0.2.0" }),
    /Ed25519 signature verification failed/,
  );
  assert.throws(
    () => verifyMinisignSignature({ data: payload, signature: sigPrehashed, pubkey: other.encodedPubkey, expectedVersion: "0.2.0" }),
    /does not match/,
  );
  assert.throws(
    () => verifyMinisignSignature({ data: payload, signature: sigPrehashed, pubkey: pair.encodedPubkey, expectedVersion: "0.9.0" }),
    /does not match release version 0\.9\.0/,
  );

  // Tamper trusted comment without re-signing global signature
  const decoded = Buffer.from(sigPrehashed, "base64").toString("utf8").replace("version:0.2.0", "version:0.2.1");
  const tamperedCommentSig = Buffer.from(decoded, "utf8").toString("base64");
  assert.throws(
    () => verifyMinisignSignature({ data: payload, signature: tamperedCommentSig, pubkey: pair.encodedPubkey, expectedVersion: "0.2.1" }),
    /global signature verification failed/,
  );
});

const assets = [
  { name: "oh-my-voice-type_0.2.0_x64-setup.exe" },
  { name: "oh-my-voice-type_0.2.0_x64-setup.exe.sig" },
  { name: "oh-my-voice-type_universal.app.tar.gz" },
  { name: "oh-my-voice-type_universal.app.tar.gz.sig" },
  { name: "oh-my-voice-type_0.2.0_universal.dmg" },
];

test("buildManifest covers Windows NSIS and every macOS updater target with cryptographic verification", () => {
  const pair = makeMinisignPair();
  const artifacts = {
    "oh-my-voice-type_0.2.0_x64-setup.exe": Buffer.from("win-nsis-bytes"),
    "oh-my-voice-type_universal.app.tar.gz": Buffer.from("mac-tarball-bytes"),
  };
  const signatures = {
    "oh-my-voice-type_0.2.0_x64-setup.exe.sig": `${makeMinisignSig({ ...pair, data: artifacts["oh-my-voice-type_0.2.0_x64-setup.exe"] })}\n`,
    "oh-my-voice-type_universal.app.tar.gz.sig": makeMinisignSig({ ...pair, data: artifacts["oh-my-voice-type_universal.app.tar.gz"] }),
  };

  const manifest = buildManifest({
    repository: "owner/repo",
    tag: "v0.2.0",
    notes: " Notes ",
    assets,
    signatures,
    pubkey: pair.encodedPubkey,
    artifacts,
    pubDate: new Date("2026-11-26T00:00:00Z"),
  });
  assert.equal(manifest.version, "0.2.0");
  assert.equal(manifest.notes, "Notes");
  assert.equal(manifest.pub_date, "2026-11-26T00:00:00.000Z");
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "darwin-aarch64", "darwin-universal", "darwin-x86_64", "windows-x86_64", "windows-x86_64-nsis",
  ]);
  assert.deepEqual(manifest.platforms["windows-x86_64"], {
    signature: signatures["oh-my-voice-type_0.2.0_x64-setup.exe.sig"].trim(),
    url: "https://github.com/owner/repo/releases/download/v0.2.0/oh-my-voice-type_0.2.0_x64-setup.exe",
  });
  assert.equal(manifest.platforms["darwin-universal"].signature, signatures["oh-my-voice-type_universal.app.tar.gz.sig"]);

  assert.throws(
    () => buildManifest({
      repository: "owner/repo",
      tag: "v0.2.0",
      assets,
      signatures,
      pubkey: pair.encodedPubkey,
      artifacts: { ...artifacts, "oh-my-voice-type_0.2.0_x64-setup.exe": Buffer.from("corrupted") },
    }),
    /Signature verification failed for oh-my-voice-type_0\.2\.0_x64-setup\.exe/,
  );
});

test("buildManifest refuses a release missing a platform or empty signatures", () => {
  const signatures = {
    "oh-my-voice-type_0.2.0_x64-setup.exe.sig": "win-sig",
    "oh-my-voice-type_universal.app.tar.gz.sig": "mac-sig",
  };
  const windowsOnly = assets.filter((asset) => !asset.name.includes(".app.tar.gz"));
  assert.throws(
    () => buildManifest({ repository: "o/r", tag: "v0.2.0", assets: windowsOnly, signatures }),
    /missing signed updater bundles: \.app\.tar\.gz/,
  );
  assert.throws(
    () => buildManifest({ repository: "o/r", tag: "v0.2.0", assets, signatures: { ...signatures, "oh-my-voice-type_universal.app.tar.gz.sig": " " } }),
    /is empty/,
  );
});

test("buildChecksums sorts entries and emits standard sha256sum lines", () => {
  const text = buildChecksums([
    { name: "b.dmg", data: Buffer.from("hello") },
    { name: "a.exe", data: Buffer.from("world") },
  ]);
  const lines = text.trim().split("\n");
  assert.equal(lines.length, 2);
  assert.equal(lines[0], `${createHash("sha256").update("world").digest("hex")}  a.exe`);
  assert.equal(lines[1], `${createHash("sha256").update("hello").digest("hex")}  b.dmg`);
});

function setupTempRepo(extraChangelog = "") {
  const work = mkdtempSync(join(tmpdir(), "ovt-release-"));
  for (const file of [
    ".gitattributes",
    "Cargo.toml",
    "Cargo.lock",
    "CHANGELOG.md",
    "apps/desktop/package.json",
    "apps/desktop/package-lock.json",
    "apps/desktop/src-tauri/tauri.conf.json",
    "scripts/release.mjs",
  ]) {
    mkdirSync(dirname(join(work, file)), { recursive: true });
    cpSync(join(root, file), join(work, file));
  }
  if (extraChangelog) {
    writeFileSync(join(work, "CHANGELOG.md"), `${readFileSync(join(work, "CHANGELOG.md"), "utf8")}\n${extraChangelog}\n`);
  }
  const git = (...args) => execFileSync("git", args, { cwd: work, encoding: "utf8" }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "test@example.invalid");
  git("config", "user.name", "test");
  git("config", "commit.gpgsign", "false");
  git("config", "tag.gpgsign", "false");
  git("add", ".");
  git("commit", "-q", "-m", "init");
  return { work, git };
}

test("release.mjs tag tags current version without modifying files", () => {
  const { work, git } = setupTempRepo();
  try {
    const before = execFileSync("node", ["scripts/release.mjs", "check", "--changelog"], { cwd: work, encoding: "utf8" }).trim();
    const headBefore = git("rev-parse", "HEAD");
    execFileSync("node", ["scripts/release.mjs", "tag"], { cwd: work, encoding: "utf8" });
    assert.equal(git("rev-parse", "HEAD"), headBefore);
    assert.equal(git("tag", "--list"), `v${before}`);
    assert.equal(git("status", "--porcelain"), "");
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("release.mjs bumps every manifest, enforces changelog/branch/remote sync, commits and tags", () => {
  const current = execFileSync("node", ["scripts/release.mjs", "check"], { cwd: root, encoding: "utf8" }).trim();
  const expected = nextVersion(current, "minor");
  const { work, git } = setupTempRepo(`## [${expected}] - 2026-12-01\n\n- Added release checks.`);
  const bare = mkdtempSync(join(tmpdir(), "ovt-origin-"));
  try {
    execFileSync("git", ["init", "-q", "--bare"], { cwd: bare });
    git("remote", "add", "origin", bare);
    git("push", "-q", "-u", "origin", "main");

    // Refuses when changelog entry is missing for a different version
    assert.throws(
      () => execFileSync("node", ["scripts/release.mjs", "patch"], { cwd: work, stdio: "pipe" }),
      /CHANGELOG\.md has no section/,
    );

    // Refuses on a non-main branch
    git("checkout", "-q", "-b", "feature/test");
    assert.throws(
      () => execFileSync("node", ["scripts/release.mjs", "minor"], { cwd: work, stdio: "pipe" }),
      /Releases must be cut from main/,
    );
    git("checkout", "-q", "main");

    // Refuses when local main is ahead of origin/main
    writeFileSync(join(work, "unpushed.txt"), "ahead\n");
    git("add", "unpushed.txt");
    git("commit", "-q", "-m", "ahead");
    assert.throws(
      () => execFileSync("node", ["scripts/release.mjs", "minor"], { cwd: work, stdio: "pipe" }),
      /not synced with origin\/main/,
    );
    git("push", "-q", "origin", "main");

    // Succeeds once synced on main with CHANGELOG entry
    execFileSync("node", ["scripts/release.mjs", "minor"], { cwd: work, encoding: "utf8" });

    assert.equal(execFileSync("node", ["scripts/release.mjs", "check"], { cwd: work, encoding: "utf8" }).trim(), expected);
    assert.equal(git("tag", "--list"), `v${expected}`);
    assert.equal(git("log", "-1", "--format=%s"), `Release v${expected}`);
    assert.equal(git("status", "--porcelain"), "");
    // Only version lines change; dependency entries stay untouched.
    const diff = git("diff", "--numstat", "HEAD~1");
    for (const line of diff.split("\n")) {
      const [added, removed, file] = line.split("\t");
      const allowed = file.endsWith("Cargo.lock") || file.endsWith("package-lock.json") ? 2 : 1;
      assert.ok(Number(added) === allowed && Number(removed) === allowed, `unexpected change in ${file}: ${line}`);
    }
    assert.match(readFileSync(join(work, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"), new RegExp(`"version": "${expected}"`));

    writeFileSync(join(work, "Cargo.toml"), readFileSync(join(work, "Cargo.toml"), "utf8").replace(expected, "9.9.9"));
    assert.throws(() => execFileSync("node", ["scripts/release.mjs", "check"], { cwd: work, stdio: "pipe" }), /Version mismatch/);
  } finally {
    rmSync(work, { recursive: true, force: true });
    rmSync(bare, { recursive: true, force: true });
  }
});
