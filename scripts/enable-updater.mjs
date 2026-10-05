// Turns on the Tauri updater once updater keys exist (spec section 17).
//
//   npm run tauri signer generate -- -w "$env:USERPROFILE\.tauri\kikitori.key"   (PowerShell)
//   node scripts/enable-updater.mjs <github-owner> [path/to/kikitori.key.pub]
//
// Give the key a full path: the Tauri CLI does not expand `~`, and PowerShell and cmd don't either.
//
// Writes the public key, the GitHub Releases endpoint and the passive Windows install mode
// into src-tauri/tauri.conf.json, and makes `tauri build` produce signed update bundles.

import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const [owner, pubPath = join(homedir(), ".tauri", "kikitori.key.pub")] = process.argv.slice(2);
if (!owner || !/^[A-Za-z0-9-]+$/.test(owner)) {
  console.error("Usage: node scripts/enable-updater.mjs <github-owner> [path/to/kikitori.key.pub]");
  process.exit(1);
}

const pubkey = readFileSync(pubPath, "utf8").trim();
const confPath = join("src-tauri", "tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));

conf.bundle.createUpdaterArtifacts = true;
conf.plugins = {
  ...conf.plugins,
  updater: {
    pubkey,
    endpoints: [`https://github.com/${owner}/kikitori/releases/latest/download/latest.json`],
    windows: { installMode: "passive" },
  },
};
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

console.log(`Updater enabled in ${confPath} for github.com/${owner}/kikitori.
Next:
  1. In the GitHub repository, Settings → Secrets and variables → Actions, add
     TAURI_SIGNING_PRIVATE_KEY           the contents of ${pubPath.replace(/\.pub$/, "")}
     TAURI_SIGNING_PRIVATE_KEY_PASSWORD  the password you chose
  2. For a local release build, set the same two environment variables first.
Keep the private key safe: without it you can't publish updates that installed copies accept.`);
