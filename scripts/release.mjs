#!/usr/bin/env node
// Keeps the app version in sync across every manifest and cuts a release tag.
//
//   node scripts/release.mjs check [--changelog]
//   node scripts/release.mjs notes [version]
//   node scripts/release.mjs tag [--push] [--dry-run] [--check] [--allow-branch]
//   node scripts/release.mjs <version|patch|minor|major> [--push] [--dry-run] [--check] [--allow-branch]
//
// Bumping rewrites the files, commits "Release vX.Y.Z" and creates an annotated
// tag. `tag` creates an annotated tag for the current manifest version without
// bumping. With --push it pushes the branch/tag, starting the GitHub release workflow.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;
const CRATES = ["ovt-core", "ovt-desktop"];

const files = {
  cargo: join(root, "Cargo.toml"),
  cargoLock: join(root, "Cargo.lock"),
  pkg: join(root, "apps/desktop/package.json"),
  pkgLock: join(root, "apps/desktop/package-lock.json"),
  tauri: join(root, "apps/desktop/src-tauri/tauri.conf.json"),
};
const changelogPath = join(root, "CHANGELOG.md");

const read = (file) => readFileSync(file, "utf8");
const rel = (file) => relative(root, file).replaceAll("\\", "/");
const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function workspaceVersion(text) {
  const section = text.match(/\[workspace\.package\][^[]*/);
  const version = section?.[0].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error("Cargo.toml has no [workspace.package] version");
  return version;
}

function lockVersion(text, name) {
  const match = text.match(new RegExp(`\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = "([^"]+)"`));
  if (!match) throw new Error(`Cargo.lock has no entry for ${name}`);
  return match[1];
}

export function readVersions() {
  const cargoLock = read(files.cargoLock);
  const pkgLock = JSON.parse(read(files.pkgLock));
  return {
    [rel(files.cargo)]: workspaceVersion(read(files.cargo)),
    ...Object.fromEntries(CRATES.map((name) => [`${rel(files.cargoLock)} (${name})`, lockVersion(cargoLock, name)])),
    [rel(files.pkg)]: JSON.parse(read(files.pkg)).version,
    [rel(files.pkgLock)]: pkgLock.version,
    [`${rel(files.pkgLock)} (packages[""])`]: pkgLock.packages?.[""]?.version,
    [rel(files.tauri)]: JSON.parse(read(files.tauri)).version,
  };
}

/**
 * Extracts the Markdown body for `version` from CHANGELOG.md.
 * Expects a heading like `## [0.1.0] - 2026-11-27` or `## [0.1.0]`.
 */
export function extractChangelogSection(markdown, version) {
  const normalized = version.replace(/^v/, "");
  const heading = new RegExp(`^##[ \\t]+\\[?v?${escapeRegExp(normalized)}\\]?(?:[ \\t]+-[ \\t]+[^\\r\\n]*)?[ \\t]*$`, "m");
  const match = heading.exec(markdown);
  if (!match) {
    throw new Error(`CHANGELOG.md has no section for version ${normalized}`);
  }
  const afterHeading = markdown.slice(match.index + match[0].length);
  const nextHeading = afterHeading.search(/\r?\n##\s+/);
  const section = (nextHeading === -1 ? afterHeading : afterHeading.slice(0, nextHeading)).trim();
  if (!section) {
    throw new Error(`CHANGELOG.md section for version ${normalized} is empty`);
  }
  return section;
}

export function readChangelogNotes(version, path = changelogPath) {
  if (!existsSync(path)) {
    throw new Error(`CHANGELOG.md not found at ${rel(path)}`);
  }
  return extractChangelogSection(read(path), version);
}

function check({ requireChangelog = false } = {}) {
  const versions = readVersions();
  const unique = [...new Set(Object.values(versions))];
  if (unique.length !== 1 || !unique[0]) {
    const lines = Object.entries(versions).map(([file, version]) => `  ${file}: ${version}`);
    throw new Error(`Version mismatch:\n${lines.join("\n")}\nRun: node scripts/release.mjs <version>`);
  }
  const version = unique[0];
  const tag = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME : undefined;
  if (tag && tag !== `v${version}`) {
    throw new Error(`Tag ${tag} does not match app version v${version}`);
  }
  if (requireChangelog) {
    readChangelogNotes(version);
  }
  return version;
}

export function nextVersion(current, bump) {
  if (SEMVER.test(bump)) return bump;
  const [major, minor, patch] = current.split("-")[0].split(".").map(Number);
  switch (bump) {
    case "major": return `${major + 1}.0.0`;
    case "minor": return `${major}.${minor + 1}.0`;
    case "patch": return `${major}.${minor}.${patch + 1}`;
    default: throw new Error(`"${bump}" is not a semver version or one of patch, minor, major`);
  }
}

function replaceOnce(text, pattern, replacement, label) {
  let count = 0;
  const result = text.replace(pattern, (...args) => {
    count += 1;
    return typeof replacement === "function" ? replacement(...args) : replacement;
  });
  if (count !== 1) throw new Error(`Expected exactly one version in ${label}, found ${count}`);
  return result;
}

function writeVersion(version) {
  const cargo = read(files.cargo);
  writeFileSync(files.cargo, replaceOnce(
    cargo,
    /(\[workspace\.package\][^[]*?^version\s*=\s*")[^"]+(")/m,
    (_, head, tail) => `${head}${version}${tail}`,
    "Cargo.toml",
  ));

  let lock = read(files.cargoLock);
  for (const name of CRATES) {
    lock = replaceOnce(
      lock,
      new RegExp(`(\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = ")[^"]+(")`),
      (_, head, tail) => `${head}${version}${tail}`,
      `Cargo.lock (${name})`,
    );
  }
  writeFileSync(files.cargoLock, lock);

  // Rewrite only the version fields so formatting and key order are untouched.
  const pkg = read(files.pkg);
  writeFileSync(files.pkg, replaceOnce(pkg, /^(  "version": ")[^"]+(")/m, (_, head, tail) => `${head}${version}${tail}`, rel(files.pkg)));

  const pkgLock = read(files.pkgLock);
  const top = replaceOnce(pkgLock, /^(  "version": ")[^"]+(")/m, (_, head, tail) => `${head}${version}${tail}`, rel(files.pkgLock));
  writeFileSync(files.pkgLock, replaceOnce(
    top,
    /("packages": \{\r?\n    "": \{\r?\n      "name": "[^"]+",\r?\n      "version": ")[^"]+(")/,
    (_, head, tail) => `${head}${version}${tail}`,
    `${rel(files.pkgLock)} packages[""]`,
  ));

  const tauri = read(files.tauri);
  writeFileSync(files.tauri, replaceOnce(tauri, /^(  "version": ")[^"]+(")/m, (_, head, tail) => `${head}${version}${tail}`, rel(files.tauri)));
}

function git(args, options = {}) {
  const output = execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: options.stdio ?? "pipe" });
  return (output ?? "").trim();
}

function hasOriginRemote() {
  try {
    git(["remote", "get-url", "origin"]);
    return true;
  } catch {
    return false;
  }
}

function ensureReleasePreconditions(version, { allowBranch }) {
  const tag = `v${version}`;
  const dirty = git(["status", "--porcelain", "--untracked-files=no"]);
  if (dirty) throw new Error(`Commit or stash your changes first:\n${dirty}`);
  if (git(["tag", "--list", tag])) throw new Error(`Tag ${tag} already exists`);

  const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
  if (!allowBranch && branch !== "main") {
    throw new Error(`Releases must be cut from main (currently on ${branch}). Pass --allow-branch to override.`);
  }

  if (!allowBranch && hasOriginRemote()) {
    git(["fetch", "--quiet", "origin", "main"]);
    const localHead = git(["rev-parse", "HEAD"]);
    const remoteHead = git(["rev-parse", "refs/remotes/origin/main"]);
    if (localHead !== remoteHead) {
      throw new Error(`Local main (${localHead.slice(0, 8)}) is not synced with origin/main (${remoteHead.slice(0, 8)}). Pull or push first.`);
    }
  }

  readChangelogNotes(version);
  return { branch, tag };
}

function runLocalChecks() {
  const desktopDir = join(root, "apps/desktop");
  const steps = [
    ["cargo", ["fmt", "--all", "--check"], root, false],
    ["cargo", ["clippy", "-p", "ovt-core", "-p", "ovt-desktop", "--", "-D", "warnings"], root, false],
    ["cargo", ["test", "-p", "ovt-core", "-p", "ovt-desktop"], root, false],
    ["npm", ["test"], desktopDir, process.platform === "win32"],
    ["npm", ["run", "build"], desktopDir, process.platform === "win32"],
    [process.execPath, ["--test", "scripts/release.test.mjs"], root, false],
  ];
  for (const [cmd, args, cwd, shell] of steps) {
    console.log(`> ${cmd} ${args.join(" ")}`);
    execFileSync(cmd, args, { cwd, shell, stdio: "inherit" });
  }
}

function tagCurrent({ push, dryRun, runCheck, allowBranch }) {
  const version = check();
  const { branch, tag } = ensureReleasePreconditions(version, { allowBranch });
  console.log(`Tagging current version ${version} (${tag}) on ${branch}`);
  if (dryRun) {
    console.log("Dry run: no tag created.");
    return;
  }
  if (runCheck) runLocalChecks();

  git(["tag", "-a", tag, "-m", `oh-my-voice-type ${tag}`]);
  console.log(`Created tag ${tag}.`);

  if (push) {
    git(["push", "origin", tag], { stdio: "inherit" });
    console.log(`Pushed ${tag}. The release workflow is now building the installers.`);
  } else {
    console.log(`Push when ready:\n  git push origin ${tag}`);
  }
}

function bump(target, { push, dryRun, runCheck, allowBranch }) {
  const current = check();
  const version = nextVersion(current, target);
  if (version === current) {
    throw new Error(`Version is already ${version}. Use "node scripts/release.mjs tag" to tag the current version.`);
  }
  const { branch, tag } = ensureReleasePreconditions(version, { allowBranch });

  console.log(`Releasing ${current} -> ${version} on ${branch}`);
  if (dryRun) {
    console.log("Dry run: no files changed.");
    return;
  }
  if (runCheck) runLocalChecks();

  writeVersion(version);
  if (check() !== version) throw new Error("Version rewrite did not apply to every file");

  git(["add", ...Object.values(files)]);
  git(["commit", "-m", `Release ${tag}`], { stdio: "inherit" });
  git(["tag", "-a", tag, "-m", `oh-my-voice-type ${tag}`]);
  console.log(`Created commit and tag ${tag}.`);

  if (push) {
    git(["push", "--atomic", "origin", branch, tag], { stdio: "inherit" });
    console.log(`Pushed ${branch} and ${tag}. The release workflow is now building the installers.`);
  } else {
    console.log(`Push when ready:\n  git push --atomic origin ${branch} ${tag}`);
  }
}

function main(argv) {
  const [command, ...rest] = argv;
  if (!command || command === "--help" || command === "-h") {
    console.log(
      "Usage: node scripts/release.mjs check [--changelog] | notes [version] | tag | <version|patch|minor|major> [--push] [--dry-run] [--check] [--allow-branch]",
    );
    return;
  }
  if (command === "check") {
    const version = check({ requireChangelog: rest.includes("--changelog") });
    console.log(version);
    if (process.env.GITHUB_OUTPUT) {
      writeFileSync(process.env.GITHUB_OUTPUT, `version=${version}\ntag=v${version}\n`, { flag: "a" });
    }
    return;
  }
  if (command === "notes") {
    const version = rest[0] && !rest[0].startsWith("--") ? rest[0] : check();
    console.log(readChangelogNotes(version));
    return;
  }
  const options = {
    push: rest.includes("--push"),
    dryRun: rest.includes("--dry-run"),
    runCheck: rest.includes("--check"),
    allowBranch: rest.includes("--allow-branch"),
  };
  if (command === "tag") {
    tagCurrent(options);
    return;
  }
  bump(command, options);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
