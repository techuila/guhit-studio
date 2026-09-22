// Sets the app version in the three places that carry it, from one argument.
//
//   node scripts/bump-version.mjs 0.1.1
//
// src-tauri/tauri.conf.json is the source of truth: it is the version the
// updater compares against latest.json. package.json and the workspace
// Cargo.toml mirror it so the crate and the npm package do not drift.
// Prints the git tag to create. Release steps: docs/RELEASING.md.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: node scripts/bump-version.mjs <major.minor.patch>   (for example 0.1.1)");
  process.exit(2);
}

/** Replaces the first match of `pattern` in a file. Fails loudly if it is not there. */
function edit(relPath, pattern, replacement) {
  const path = join(root, relPath);
  const before = readFileSync(path, "utf8");
  if (!pattern.test(before)) {
    console.error(`bump-version: no version line matched in ${relPath}. Nothing was written.`);
    process.exit(1);
  }
  const after = before.replace(pattern, replacement);
  writeFileSync(path, after);
  return before !== after;
}

// The "version" key at the top level of each JSON file, and the one inside
// [workspace.package] in Cargo.toml.
edit("src-tauri/tauri.conf.json", /("version":\s*)"[^"]+"/, `$1"${version}"`);
edit("package.json", /("version":\s*)"[^"]+"/, `$1"${version}"`);
edit("Cargo.toml", /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"]+"/, `$1"${version}"`);

console.log(`version set to ${version} in src-tauri/tauri.conf.json, package.json and Cargo.toml`);
console.log("");
console.log("Next, after Cargo.lock is refreshed and the change is committed:");
console.log(`  git tag v${version}`);
console.log(`  git push origin v${version}`);
