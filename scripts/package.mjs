#!/usr/bin/env node
/**
 * Build the app and the NSIS installer with Authenticode signing enabled.
 *
 * Tauri reads the certificate thumbprint from `bundle.windows`, signs
 * `islandwin.exe` before NSIS packs it and signs the setup afterwards, so both
 * binaries carry a signature. The thumbprint is injected with `--config` and
 * never written to `tauri.conf.json`: the certificate stays in the Windows
 * store and the repository keeps no key material.
 *
 * Run with `npm run package`. Extra arguments are forwarded to `tauri build`.
 */
import { execFileSync } from "node:child_process";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ensureSignToolOnPath,
  resolveSigningCertificate,
  signFile,
  tauriSigningConfig,
} from "./lib/signing.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CLI = join(ROOT, "node_modules", "@tauri-apps", "cli", "tauri.js");
const APP_BINARY = join(ROOT, "src-tauri", "target", "release", "islandwin.exe");

const certificate = resolveSigningCertificate();
const args = ["build", ...process.argv.slice(2)];
let configPath;

if (certificate) {
  ensureSignToolOnPath();
  configPath = join(tmpdir(), "islandwin-signing.json");
  writeFileSync(
    configPath,
    `${JSON.stringify({ bundle: { windows: tauriSigningConfig(certificate) } }, null, 2)}\n`,
  );
  args.push("--config", configPath);
  const trust = certificate.selfSigned ? "self-signed, Windows will not trust it" : "trusted";
  console.log(`signing with ${certificate.subject} (${trust})`);
} else {
  console.warn("no code-signing certificate found; building an unsigned installer");
}

try {
  execFileSync(process.execPath, [CLI, ...args], {
    cwd: ROOT,
    stdio: "inherit",
    env: process.env,
  });

  // Tauri signs the binary it hands to NSIS, then restores the unpatched build
  // output, so the loose islandwin.exe loses the signature the installer keeps.
  // Sign it again for the build directory that releases are assembled from.
  if (certificate && existsSync(APP_BINARY)) {
    signFile(APP_BINARY, certificate);
    console.log(`signed ${APP_BINARY}`);
  }
} finally {
  if (configPath) rmSync(configPath, { force: true });
}
