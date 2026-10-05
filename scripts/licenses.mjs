// Writes the licence texts of everything the installer ships besides Kikitori's own code: the
// Rust crates linked into the exe, the whisper.cpp sources built from whisper-rs-sys, and the
// frontend packages Vite bundles. MIT, BSD and Apache-2.0 ask for their notices to travel with
// the binary. Typst's LICENSE and NOTICE and the Vulkan loader's licence ship as their own files
// in the same licenses\ folder.
//
//   node scripts/licenses.mjs
//
// writes src-tauri/resources/licenses/THIRD-PARTY-NOTICES.txt (git-ignored). `build` in
// scripts/tauri-gpu.mjs runs it before every installer build.

import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "src-tauri", "resources", "licenses", "THIRD-PARTY-NOTICES.txt");

// LICENSE-MIT, license-apache-2.0, COPYING, COPYRIGHT, NOTICE, UNLICENSE and the like.
// LICENSE.spdx files are machine-readable metadata, not licence texts.
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense)/i;

function licenseFiles(dir) {
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f) && !f.endsWith(".spdx") && statSync(join(dir, f)).isFile())
    .sort()
    .map((f) => join(dir, f));
}

// Crates linked into the exe of the Windows GPU build. Build scripts and proc macros only run
// while compiling, so crates that only they depend on are left out.
function rustCrates() {
  const args = ["metadata", "--format-version", "1", "--locked", "--features", "gpu-vulkan"];
  args.push("--filter-platform", "x86_64-pc-windows-msvc");
  const r = spawnSync("cargo", args, { cwd: join(ROOT, "src-tauri"), encoding: "utf8", maxBuffer: 1 << 30 });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`cargo metadata failed:\n${r.stderr}`);
  const meta = JSON.parse(r.stdout);
  const packages = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const crates = [];
  const seen = new Set([meta.resolve.root]);
  const queue = [meta.resolve.root];
  while (queue.length > 0) {
    for (const dep of nodes.get(queue.pop()).deps) {
      if (seen.has(dep.pkg) || !dep.dep_kinds.some((k) => k.kind === null)) continue;
      seen.add(dep.pkg);
      const p = packages.get(dep.pkg);
      if (p.targets.some((t) => t.kind.includes("proc-macro"))) continue;
      crates.push(p);
      queue.push(dep.pkg);
    }
  }
  return crates;
}

function crateEntry(p) {
  const dir = dirname(p.manifest_path);
  const files = licenseFiles(dir);
  const declared = p.license_file && resolve(dir, p.license_file);
  if (declared && existsSync(declared) && !files.includes(declared)) files.push(declared);
  return { label: `${p.name} ${p.version}`, license: p.license ?? "unknown", repository: p.repository, files };
}

// whisper-rs-sys itself is Unlicense, but it compiles the whisper.cpp and ggml sources it carries,
// which are MIT.
function whisperCpp(crates) {
  const sys = crates.find((p) => p.name === "whisper-rs-sys");
  const file = join(dirname(sys.manifest_path), "whisper.cpp", "LICENSE");
  if (!existsSync(file)) throw new Error(`${file} is missing: check where whisper-rs-sys keeps whisper.cpp`);
  return { label: `whisper.cpp and ggml (built from whisper-rs-sys ${sys.version})`, license: "MIT", files: [file] };
}

// Packages Vite can bundle: the dependencies in package.json and theirs, found the way Node
// resolves them, so pnpm's symlinked node_modules and npm's flat one both work. Tailwind is a
// dev dependency, but its base styles end up in the bundled CSS.
function frontendPackages() {
  const found = new Map();
  const visit = (name, from) => {
    const dir = resolvePackage(name, from);
    if (!dir) throw new Error(`${name} is not installed: run pnpm install`);
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const label = `${pkg.name} ${pkg.version}`;
    if (found.has(label)) return;
    const repository = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
    found.set(label, {
      label,
      license: typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? "unknown"),
      repository: repository?.replace(/^git\+/, "").replace(/\.git$/, ""),
      files: licenseFiles(dir),
    });
    for (const dep of Object.keys(pkg.dependencies ?? {})) visit(dep, dir);
  };
  const root = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  for (const dep of [...Object.keys(root.dependencies), "tailwindcss"]) visit(dep, ROOT);
  return [...found.values()];
}

function resolvePackage(name, from) {
  for (let dir = from; ; dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", name);
    if (existsSync(join(candidate, "package.json"))) return realpathSync(candidate);
    if (dirname(dir) === dir) return undefined;
  }
}

const RULE = "-".repeat(80);

const normalize = (text) =>
  text
    .replace(/^﻿/, "")
    .replace(/\r\n?/g, "\n")
    .replace(/[ \t]+$/gm, "")
    .trim();

// "a 1.0, b 2.0, ..." broken into lines of at most `width` characters.
function wrapList(items, width = 80) {
  const lines = [""];
  for (const item of items) {
    const line = lines[lines.length - 1];
    if (line && line.length + 2 + item.length > width) {
      lines[lines.length - 1] = `${line},`;
      lines.push(item);
    } else {
      lines[lines.length - 1] = line ? `${line}, ${item}` : item;
    }
  }
  return lines.join("\n");
}

// One block per distinct licence text, listing every package that ships it, then the packages
// that ship no licence file at all.
function section(title, entries) {
  const texts = new Map();
  for (const entry of entries) {
    for (const file of entry.files) {
      const text = normalize(readFileSync(file, "utf8"));
      if (!texts.has(text)) texts.set(text, new Set());
      texts.get(text).add(entry.label);
    }
  }
  const blocks = [...texts]
    .map(([text, labels]) => ({ text, labels: [...labels].sort() }))
    .sort((a, b) => a.labels[0].localeCompare(b.labels[0]));
  const out = [`${"=".repeat(80)}\n${title}\n${"=".repeat(80)}`];
  for (const { text, labels } of blocks) out.push(`${RULE}\nUsed by: ${wrapList(labels)}\n\n${text}`);
  const bare = entries.filter((e) => e.files.length === 0).sort((a, b) => a.label.localeCompare(b.label));
  if (bare.length > 0) {
    const lines = bare.map((e) => `${e.label}: ${e.license}${e.repository ? `, ${e.repository}` : ""}`);
    out.push(`${RULE}\nThese packages ship no licence file. Each line gives the licence it declares and\nwhere its source is.\n\n${lines.join("\n")}`);
  }
  return out.join("\n\n");
}

export function thirdPartyNotices() {
  const crates = rustCrates();
  const header = `Third-party software in Kikitori
================================

Kikitori itself is under the MIT licence. Its installer also contains the software below; each
licence text is followed by the packages it covers. Typst's licence and NOTICE and the Vulkan
loader's licence are the other files in this folder.`;
  return [
    header,
    section("whisper.cpp", [whisperCpp(crates)]),
    section("Rust crates", crates.map(crateEntry)),
    section("JavaScript packages", frontendPackages()),
  ].join("\n\n\n") + "\n";
}

export function writeThirdPartyNotices() {
  writeFileSync(OUT, thirdPartyNotices());
  return OUT;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(`Wrote ${writeThirdPartyNotices()}`);
}
