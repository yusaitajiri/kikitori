// Runs `tauri dev` or `tauri build` with the Vulkan GPU backend.
//
// whisper.cpp's Vulkan build compiles a nested CMake project whose MSBuild paths exceed
// Windows' 260-character limit under a deep checkout, so the cargo target directory defaults
// to a short folder in the user profile. Set CARGO_TARGET_DIR to override it.
//
// `build` also bundles Khronos' Vulkan loader (vulkan-1.dll) next to the exe, so the app
// starts on PCs without a GPU driver and falls back to the CPU (spec section 17), and writes
// the third-party licence list the installer ships (scripts/licenses.mjs).

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { writeThirdPartyNotices } from "./licenses.mjs";

const VULKAN_RT = {
  url: "https://sdk.lunarg.com/sdk/download/1.4.363.0/windows/VulkanRT-X64-1.4.363.0-Components.zip",
  zipSha256: "a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a",
  dllSha256: "e1fcfc9489beefa6a6d9d11d6c7517f6a2b7f16e0d3fb8f4103bee0210f314a3",
  folder: "VulkanRT-X64-1.4.363.0-Components",
};

const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

async function ensureVulkanRuntime() {
  const dir = join("src-tauri", "resources", "vulkan");
  const dll = join(dir, "vulkan-1.dll");
  if (existsSync(dll) && sha256(dll) === VULKAN_RT.dllSha256) return;
  console.log(`Fetching the Vulkan runtime from ${VULKAN_RT.url}`);
  const res = await fetch(VULKAN_RT.url);
  if (!res.ok) throw new Error(`download failed: HTTP ${res.status}`);
  const zip = join(tmpdir(), "kikitori-vulkan-rt.zip");
  writeFileSync(zip, Buffer.from(await res.arrayBuffer()));
  if (sha256(zip) !== VULKAN_RT.zipSha256) throw new Error("Vulkan runtime checksum mismatch");
  const out = join(tmpdir(), "kikitori-vulkan-rt");
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  // Windows 10+ ships bsdtar, which reads zip files. Call it by path: Git Bash puts GNU tar,
  // which can't, first on PATH.
  const tarExe = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
  const tar = spawnSync(tarExe, ["-xf", zip, "-C", out], { stdio: "inherit" });
  if (tar.status !== 0) throw new Error("extracting the Vulkan runtime failed");
  mkdirSync(dir, { recursive: true });
  copyFileSync(join(out, VULKAN_RT.folder, "x64", "vulkan-1.dll"), dll);
  copyFileSync(join(out, VULKAN_RT.folder, "VulkanRT-License.txt"), join(dir, "VulkanRT-License.txt"));
  if (sha256(dll) !== VULKAN_RT.dllSha256) throw new Error("vulkan-1.dll checksum mismatch");
}

// A terminal opened from an app that was running before the SDK was installed (Windows
// Terminal, VS Code) inherits an environment without VULKAN_SDK. Read the value the SDK
// installer saved in the registry instead.
function vulkanSdkFromRegistry() {
  const keys = ["HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment", "HKCU\\Environment"];
  for (const key of keys) {
    const r = spawnSync("reg", ["query", key, "/v", "VULKAN_SDK"], { encoding: "utf8" });
    const m = r.status === 0 && /VULKAN_SDK\s+REG_(?:EXPAND_)?SZ\s+(.+)/.exec(r.stdout);
    if (m) return m[1].trim();
  }
  return undefined;
}

const [command = "dev", ...rest] = process.argv.slice(2);
const env = { ...process.env };
env.CARGO_TARGET_DIR ??= join(homedir(), ".kt");
env.VULKAN_SDK ||= vulkanSdkFromRegistry();

if (!env.VULKAN_SDK) {
  console.error("The Vulkan SDK was not found. Install it with: winget install KhronosGroup.VulkanSDK");
  process.exit(1);
}

const args = [command, "--features", "gpu-vulkan"];
if (command === "build") {
  await ensureVulkanRuntime();
  console.log(`Wrote ${writeThirdPartyNotices()}`);
  args.push("--config", "src-tauri/tauri.gpu.conf.json");
}
console.log(`GPU build: VULKAN_SDK=${env.VULKAN_SDK} CARGO_TARGET_DIR=${env.CARGO_TARGET_DIR}`);
const result = spawnSync("npx", ["tauri", ...args, ...rest], { stdio: "inherit", env, shell: true });
process.exit(result.status ?? 1);
