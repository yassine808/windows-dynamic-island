/**
 * Run Windows PowerShell 5.1 with a module path that only contains the
 * Windows PowerShell module directories.
 *
 * When `PSModulePath` also lists PowerShell 7 modules (a side effect of some
 * shells and CI setups), 5.1 tries to load their extended type data and
 * `Microsoft.PowerShell.Security` fails with "the member ... is already
 * present", which breaks `Get-AuthenticodeSignature` for no useful reason.
 */
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

function windowsPowerShellModulePath() {
  const home = process.env.USERPROFILE;
  const systemRoot = process.env.SystemRoot || "C:\\Windows";
  return [
    home && join(home, "Documents", "WindowsPowerShell", "Modules"),
    join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "Modules"),
  ]
    .filter((path) => path && existsSync(path))
    .join(";");
}

export function runPowerShell(script) {
  return execFileSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    {
      encoding: "utf8",
      windowsHide: true,
      env: { ...process.env, PSModulePath: windowsPowerShellModulePath() },
    },
  );
}
