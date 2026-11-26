// Refuse to publish credentials or private keys: scans every tracked file (CI + local).
// Run with `node scripts/secret-scan.mjs`.
import { execFileSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";

const files = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8" }).split("\0").filter(Boolean);
const textExt = /\.(rs|ts|tsx|js|mjs|cjs|json|md|css|html|txt|yml|yaml|nsh|ps1|csv|toml|lock|gitignore)$/i;
const patterns = [
  [/gh[pousr]_[A-Za-z0-9]{36,}/, "GitHub token"],
  [/github_pat_[A-Za-z0-9_]{20,}/, "GitHub fine-grained token"],
  [/AKIA[0-9A-Z]{16}/, "AWS access key"],
  [/-----BEGIN (?:RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----/, "private key"],
  [/\bsk-[A-Za-z0-9]{24,}\b/, "API secret key"],
  [/\bxox[baprs]-[A-Za-z0-9-]{10,}\b/, "Slack token"],
  [/\bAIza[0-9A-Za-z_-]{35}\b/, "Google API key"],
  [/\bnpm_[A-Za-z0-9]{36}\b/, "npm token"],
];
const denyName = [/(^|\/)\.env(\..*)?$/, /\.(pem|key|p12|pfx|keystore)$/i, /(^|\/)id_(rsa|ed25519)$/];
const maxBytes = 5 * 1024 * 1024;

let failures = 0;
const fail = (m) => { console.log(`FAIL  ${m}`); failures++; };

for (const f of files) {
  let size = 0;
  try { size = statSync(f).size; } catch { continue; }
  if (size > maxBytes) fail(`${f} is ${(size / 1048576).toFixed(1)} MB (max 5 MB)`);
  if (denyName.some((r) => r.test(f))) fail(`${f} looks like a secret file`);
  if (size > 512 * 1024 || !textExt.test(f)) continue;
  let text = "";
  try { text = readFileSync(f, "utf8"); } catch { continue; }
  for (const [re, name] of patterns) {
    if (re.test(text)) fail(`${f}: possible ${name}`);
  }
}

console.log(failures ? `\n${failures} secret scan problem(s)` : `\nno secrets in ${files.length} tracked files`);
process.exit(failures ? 1 : 0);
