#!/usr/bin/env node
/**
 * Sign existing Windows binaries with the certificate in the Windows store.
 *
 * `npm run sign -- <file> [<file>...]`
 *
 * With no arguments it signs the current release build (the application binary
 * and the NSIS setup). Signing a setup after it was built does not sign the
 * application binary packed inside it, so a release should go through
 * `npm run package`, which signs both during the build.
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveSigningCertificate, signFile } from "./lib/signing.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const RELEASE = join(ROOT, "src-tauri", "target", "release");
const version = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

const explicit = process.argv.slice(2);
const files =
  explicit.length > 0
    ? explicit.map((file) => resolve(file))
    : [
        join(RELEASE, "islandwin.exe"),
        join(RELEASE, "bundle", "nsis", `IslandWin_${version}_x64-setup.exe`),
      ];

const missing = files.filter((file) => !existsSync(file));
if (missing.length > 0) {
  for (const file of missing) console.error(`bad ${file}: missing`);
  console.error("sign: build the release first with `npm run package`");
  process.exit(1);
}

const certificate = resolveSigningCertificate();
if (!certificate) {
  console.error("sign: no code-signing certificate found in Cert:\\CurrentUser\\My");
  process.exit(1);
}

const trust = certificate.selfSigned ? " (self-signed, Windows will not trust it)" : "";
for (const file of files) {
  signFile(file, certificate);
  console.log(`signed ${file}${trust}`);
}

execFileSync(
  process.execPath,
  [join(ROOT, "scripts", "verify-signatures.mjs"), ...files],
  { cwd: ROOT, stdio: "inherit", env: process.env },
);
