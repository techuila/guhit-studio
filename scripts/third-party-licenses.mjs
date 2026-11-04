// SPDX-License-Identifier: AGPL-3.0-only
//
// Writes THIRD_PARTY_LICENSES.md: the licenses of the Rust crates and npm
// packages that ship inside the desktop app. MIT, BSD, ISC, Apache-2.0 and
// Unicode terms ask that the notice travels with the software, so the file is
// bundled into the app (src-tauri/tauri.conf.json, bundle.resources) and
// attached to every release (.github/workflows/release.yml).
//
//   node scripts/third-party-licenses.mjs           write THIRD_PARTY_LICENSES.md
//   node scripts/third-party-licenses.mjs --check   exit 1 when the file is stale
//
// No dependency beyond Node, cargo and pnpm:
//   Rust: `cargo metadata` for each release target, then the normal
//         (not dev, not build) dependencies of the desktop shell. License
//         files are read from the crate sources cargo has downloaded.
//   npm:  `pnpm licenses list --prod --json` (run `pnpm install` first).
//   Vendored code inside npm packages (the meshopt decoder, a denoise shader,
//         xatlas): the VENDORED list below, checked against what is installed.
// Packages with no license file get their SPDX id and a link to its text.
// The output has no dates or local paths, so it only changes when a
// dependency does.

import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const outPath = join(root, "THIRD_PARTY_LICENSES.md");
const check = process.argv.includes("--check");

// The three release builds (.github/workflows/release.yml). A crate counts
// when any of them links it.
const TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"];
// The package that becomes the desktop app (src-tauri/Cargo.toml).
const ROOT_CRATE = "guhit-studio";

// Where a crate offers a choice of licenses, the one we take. r-efi (UEFI
// only, not linked on macOS or Windows) lists LGPL as an option; we take MIT.
const ELECTED = { "r-efi": "MIT" };

// Third-party code that sits inside an npm package under its own copyright.
// The package's own LICENSE does not name its author, and the build strips
// the header comment, so the notice is kept here. Texts are in
// scripts/third-party-vendored/. Each entry is checked against the installed
// package, so an upgrade that moves or drops the code fails the script
// instead of leaving a stale notice. Review three's other libs/ files (fflate,
// Draco, KTX2) if the app ever imports one: today it imports only the meshopt
// decoder from them.
const VENDORED = [
  {
    name: "meshoptimizer decoder",
    host: "three",
    file: "examples/jsm/libs/meshopt_decoder.module.js",
    marker: "Arseny Kapoulkine",
    license: "MIT",
    copyright: "Copyright (c) 2016-2026 Arseny Kapoulkine",
    source: "https://github.com/zeux/meshoptimizer",
    note: "A WebAssembly decoder for compressed meshes. The 3D asset pack loads it from three.js (src/viewer3d/scene/pack.ts); it ends up in the app's JavaScript with its header comment removed.",
    text: "meshoptimizer.txt",
  },
  {
    name: "glslSmartDeNoise",
    host: "three-gpu-pathtracer",
    file: "src/materials/fullscreen/DenoiseMaterial.js",
    marker: "Michele Morrone",
    license: "BSD-2-Clause",
    copyright: "Copyright (c) 2018-2020 Michele Morrone. All rights reserved.",
    source: "https://github.com/BrutPitt/glslSmartDeNoise",
    note: "The denoise shader of the path tracer that renders images.",
    text: "glslSmartDeNoise.txt",
  },
  {
    name: "xatlas, with thekla_atlas and Fast-BVH",
    host: "xatlas-web",
    file: "dist/xatlas-web.wasm",
    marker: null,
    license: "MIT",
    copyright: "Copyright (c) 2018-2020 Jonathan Young; (c) 2013 Thekla, Inc; (c) 2006 NVIDIA Corporation, Ignacio Castano; (c) 2012 Brandon Pelfrey",
    source: "https://github.com/jpcy/xatlas",
    note: "UV unwrapping, compiled to WebAssembly in xatlas-web and used by the path tracer. The package's own LICENSE names only the first author.",
    text: "xatlas.txt",
  },
];

const LICENSE_FILE = /^(licen[cs]e|copying|unlicen[cs]e|notice|copyright)([-_. ].*)?$/i;
const NOT_TEXT = /\.(rs|toml|json|js|mjs|cjs|ts|py|c|h|yml|yaml|sh|lock|spdx)$/i;
const MAX_TEXT_BYTES = 200 * 1024;

function fail(message) {
  console.error(`third-party-licenses: ${message}`);
  process.exit(1);
}

/** Plain code point order, so the output never depends on the locale. */
const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

/** SPDX expression as the manifests write it: "MIT/Apache-2.0" becomes "MIT OR Apache-2.0". */
function normalizeLicense(raw) {
  if (!raw) return "see license file";
  // SPDX ids never contain a slash; old manifests use it for "or".
  const s = raw.trim().replace(/\s*\/\s*/g, " OR ");
  return s.replace(/\s+or\s+/g, " OR ").replace(/\s+and\s+/g, " AND ").replace(/\s+with\s+/g, " WITH ");
}

/** The SPDX ids (licenses and exceptions) an expression mentions. */
function spdxIds(expression) {
  return expression
    .split(/\s+(?:OR|AND|WITH)\s+|[()]/)
    .map((s) => s.trim().replace(/\+$/, ""))
    .filter((s) => s && s !== "see license file");
}

/** Names only: a notice needs the author, not their mailbox. */
const stripEmail = (author) => author.replace(/\s*<[^>]*>/g, "").trim();

/** License text files in a package directory, plus the manifest's own license_file. */
function readLicenseTexts(dir, licenseFile) {
  // A missing folder would look like a package without a license file, so stop instead.
  if (!existsSync(dir)) fail(`package source not found: ${dir}. Run cargo fetch and pnpm install first.`);
  const names = new Set();
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isFile() && LICENSE_FILE.test(entry.name) && !NOT_TEXT.test(entry.name)) names.add(entry.name);
  }
  if (licenseFile && existsSync(join(dir, licenseFile))) names.add(licenseFile);
  const texts = [];
  for (const name of [...names].sort(byText)) {
    const path = join(dir, name);
    if (!statSync(path).isFile() || statSync(path).size > MAX_TEXT_BYTES) continue;
    const text = readFileSync(path, "utf8").replace(/\r\n?/g, "\n").trim();
    if (text) texts.push({ name, text });
  }
  return texts;
}

// ---------------------------------------------------------------- Rust

function collectRust() {
  const found = new Map(); // "name@version" -> package
  for (const target of TARGETS) {
    let meta;
    try {
      // Downloads the crates this target needs, so their license files are on disk.
      execFileSync("cargo", ["fetch", "--locked", "--target", target], { cwd: root, stdio: ["ignore", "ignore", "inherit"] });
      const out = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", target], {
        cwd: root,
        encoding: "utf8",
        maxBuffer: 1 << 30,
        stdio: ["ignore", "pipe", "inherit"],
      });
      meta = JSON.parse(out);
    } catch (error) {
      fail(`cargo fetch or cargo metadata failed for ${target}: ${error.message}`);
    }
    const byId = new Map(meta.packages.map((p) => [p.id, p]));
    const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
    const rootPackage = meta.packages.find((p) => p.name === ROOT_CRATE);
    if (!rootPackage) fail(`crate ${ROOT_CRATE} not found in the workspace`);
    // Follow normal dependencies only: dev and build dependencies do not ship.
    const seen = new Set([rootPackage.id]);
    const stack = [rootPackage.id];
    while (stack.length) {
      for (const dep of nodes.get(stack.pop()).deps) {
        if (!dep.dep_kinds.some((k) => k.kind === null) || seen.has(dep.pkg)) continue;
        seen.add(dep.pkg);
        stack.push(dep.pkg);
      }
    }
    for (const id of seen) {
      const pkg = byId.get(id);
      if (!pkg.source) continue; // our own workspace crates: AGPL-3.0-only, see LICENSE
      found.set(`${pkg.name}@${pkg.version}`, pkg);
    }
  }
  return [...found.values()]
    .map((pkg) => {
      const license = ELECTED[pkg.name] ?? normalizeLicense(pkg.license);
      const onCratesIo = /crates\.io/.test(pkg.source);
      return {
        ecosystem: "rust",
        name: pkg.name,
        version: pkg.version,
        license,
        url: onCratesIo ? `https://crates.io/crates/${pkg.name}/${pkg.version}` : pkg.repository || pkg.homepage || "",
        authors: (pkg.authors ?? []).map(stripEmail),
        repository: pkg.repository || "",
        texts: readLicenseTexts(dirname(pkg.manifest_path), pkg.license_file),
      };
    })
    .sort((a, b) => byText(a.name, b.name) || byText(a.version, b.version));
}

// ----------------------------------------------------------------- npm

function collectNpm() {
  let listing;
  try {
    // A fixed command string: pnpm is pnpm.cmd on Windows, which needs a shell.
    const out = execSync("pnpm licenses list --prod --json", { cwd: root, encoding: "utf8", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "inherit"] });
    listing = JSON.parse(out);
  } catch (error) {
    fail(`pnpm licenses list failed (run pnpm install first): ${error.message}`);
  }
  const packages = [];
  for (const entries of Object.values(listing)) {
    for (const entry of entries) {
      entry.versions.forEach((version, i) => {
        const dir = entry.paths[i] ?? entry.paths[0];
        packages.push({
          ecosystem: "npm",
          dir,
          name: entry.name,
          version,
          license: normalizeLicense(entry.license),
          url: `https://www.npmjs.com/package/${entry.name}/v/${version}`,
          authors: entry.author ? [stripEmail(entry.author)] : [],
          repository: entry.homepage || "",
          texts: readLicenseTexts(dir, null),
        });
      });
    }
  }
  return packages.sort((a, b) => byText(a.name, b.name) || byText(a.version, b.version));
}

// -------------------------------------------------------------- output

const rust = collectRust();
const npm = collectNpm();
const all = [...rust, ...npm];

const label = (p) => (p.ecosystem === "npm" ? `npm:${p.name}@${p.version}` : `${p.name}@${p.version}`);

// License texts, grouped by identical content so a text shared by hundreds
// of crates appears once.
const groups = new Map(); // key -> { text, files, users }
for (const pkg of all) {
  for (const { name, text } of pkg.texts) {
    const key = createHash("sha256").update(text.replace(/\s+/g, " ")).digest("hex");
    if (!groups.has(key)) groups.set(key, { text, files: new Set(), users: [] });
    const group = groups.get(key);
    group.files.add(name);
    group.users.push(label(pkg));
  }
}
const orderedGroups = [...groups.values()].sort((a, b) => b.users.length - a.users.length || byText(a.text, b.text));

// Vendored code: check it is still in the installed package, then read its text.
const vendored = VENDORED.map((v) => {
  const host = npm.find((p) => p.name === v.host);
  if (!host) fail(`vendored notice "${v.name}": npm package ${v.host} is no longer a production dependency. Update VENDORED.`);
  const path = join(host.dir, v.file);
  if (!existsSync(path)) fail(`vendored notice "${v.name}": ${v.host}/${v.file} not found. Update VENDORED.`);
  if (v.marker && !readFileSync(path, "utf8").includes(v.marker)) {
    fail(`vendored notice "${v.name}": "${v.marker}" is not in ${v.host}/${v.file} any more. Update VENDORED.`);
  }
  const text = readFileSync(join(root, "scripts", "third-party-vendored", v.text), "utf8").replace(/\r\n?/g, "\n").trim();
  return { ...v, hostLabel: `${host.name}@${host.version}`, text };
});

const withoutText = all.filter((p) => p.texts.length === 0);
const mpl = all.filter((p) => spdxIds(p.license).includes("MPL-2.0"));

const counts = new Map(); // license expression -> { rust, npm }
for (const pkg of all) {
  if (!counts.has(pkg.license)) counts.set(pkg.license, { rust: 0, npm: 0 });
  counts.get(pkg.license)[pkg.ecosystem] += 1;
}
const ids = [...new Set(all.flatMap((p) => spdxIds(p.license)))].sort(byText);

const lines = [];
const add = (...l) => lines.push(...l);

add(
  "# Third-party licenses",
  "",
  "Guhit Studio is licensed under the GNU Affero General Public License, version 3 only (see `LICENSE` and `NOTICE`). The desktop app also contains the third-party software listed here, each under its own license. The notices below are kept as the licenses require.",
  "",
  "This file is generated by `node scripts/third-party-licenses.mjs` from `Cargo.lock` and `pnpm-lock.yaml`. Do not edit it by hand.",
  "",
  `- **Rust crates (${rust.length}):** the normal dependencies of the desktop shell for macOS (Apple silicon and Intel) and Windows (x64). Dev and build tools are not listed.`,
  `- **npm packages (${npm.length}):** the production dependencies of the interface, with the packages they need.`,
  `- **Code inside npm packages (${vendored.length}):** libraries with their own copyright that three.js and other packages carry, such as the meshoptimizer decoder. Their notices are in their own section below.`,
  "- **Assets:** the 3D asset pack is CC0, see the end of this file.",
  "",
  "## Summary",
  "",
  "| License | Rust crates | npm packages |",
  "|---|---:|---:|",
);
for (const [license, n] of [...counts].sort((a, b) => b[1].rust + b[1].npm - a[1].rust - a[1].npm || byText(a[0], b[0]))) {
  add(`| ${license} | ${n.rust} | ${n.npm} |`);
}
add("", "SPDX identifiers used, with the full text of each license:", "");
for (const id of ids) add(`- \`${id}\`: https://spdx.org/licenses/${id}.html`);

add("", `## Rust crates (${rust.length})`, "", "| Crate | Version | License |", "|---|---|---|");
for (const p of rust) add(`| ${p.url ? `[${p.name}](${p.url})` : p.name} | ${p.version} | ${p.license} |`);

add("", `## npm packages (${npm.length})`, "", "| Package | Version | License |", "|---|---|---|");
for (const p of npm) add(`| [${p.name}](${p.url}) | ${p.version} | ${p.license} |`);

if (mpl.length) {
  add(
    "",
    "## Source code of MPL-2.0 components",
    "",
    "These components are under the Mozilla Public License 2.0. They are used unmodified. Their source code is available from the links in the tables above (crates.io shows the repository of each crate):",
    "",
  );
  for (const p of mpl) add(`- ${p.name} ${p.version}: ${p.url}`);
}

add(
  "",
  `## Code inside npm packages (${vendored.length})`,
  "",
  "These libraries are not npm packages of their own. They sit inside the packages named below, under their own copyright, and the app build removes their header comments. Their notices are kept here.",
);
for (const v of vendored) {
  add(
    "",
    `### ${v.name}`,
    "",
    `- Inside: ${v.hostLabel} (\`${v.file}\`)`,
    `- License: ${v.license} (https://spdx.org/licenses/${v.license}.html)`,
    `- Copyright: ${v.copyright}`,
    `- Source: ${v.source}`,
    `- ${v.note}`,
    "",
    "````text",
    v.text,
    "````",
  );
}

add(
  "",
  "## Bundled assets",
  "",
  "The 3D asset pack (HDRI sky, PBR materials and furniture models in `public/assets/pack`) is CC0 1.0, a public domain dedication, from Poly Haven, Kenney and ambientCG. No attribution is required; the sources and each asset are listed in `assets/ASSETS.md`. Text: https://creativecommons.org/publicdomain/zero/1.0/",
);

if (withoutText.length) {
  add(
    "",
    "## Packages without a license file in their download",
    "",
    "These packages state their license in their manifest but ship no license file. The SPDX link in the summary above gives the text; the authors and repository are as their manifests give them.",
    "",
  );
  for (const p of withoutText) {
    const authors = p.authors.length ? `; authors: ${p.authors.join(", ")}` : "";
    add(`- ${label(p)}: ${p.license}${authors}${p.repository ? `; ${p.repository}` : ""}`);
  }
}

add("", "## License texts", "", "Each text appears once, with the packages that ship it.");
orderedGroups.forEach((group, i) => {
  add("", `### Text ${i + 1} (${[...group.files].sort(byText).join(", ")})`, "", `Used by: ${group.users.join(", ")}`, "", "````text", group.text, "````");
});

const output = `${lines.join("\n")}\n`;

if (check) {
  const current = existsSync(outPath) ? readFileSync(outPath, "utf8").replace(/\r\n/g, "\n") : "";
  if (current !== output) fail("THIRD_PARTY_LICENSES.md is out of date. Run: node scripts/third-party-licenses.mjs");
  console.log("THIRD_PARTY_LICENSES.md is up to date.");
} else {
  writeFileSync(outPath, output);
  const kb = Math.round(Buffer.byteLength(output) / 1024);
  console.log(`Wrote THIRD_PARTY_LICENSES.md: ${rust.length} Rust crates, ${npm.length} npm packages, ${orderedGroups.length} license texts, ${kb} KB.`);
  if (withoutText.length) console.log(`${withoutText.length} package(s) ship no license file; listed by SPDX id.`);
}
