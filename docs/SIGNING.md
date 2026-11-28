# Code signing

IslandWin ships an NSIS installer. Windows SmartScreen and some antivirus
products treat an unsigned installer as suspicious, so release builds are
signed with Authenticode. This page covers how the signing is wired, how to
build a signed installer locally, and how to verify the result.

## How it works

The private key never enters the repository, a build artifact or a GitHub
secret. The certificate lives in the Windows certificate store and `signtool`
signs with it from there:

```
Cert:\CurrentUser\My  ──signtool /sha1──▶  islandwin.exe  ──NSIS──▶  setup.exe
        ▲                                        ▲                       ▲
        └──────── tauri build signs both binaries before publishing ─────┘
```

`scripts/package.mjs` resolves the certificate, injects its thumbprint into
`bundle.windows` with a temporary `--config` file, and runs `tauri build`.
Tauri signs the application binary before NSIS packs it and signs the setup
afterwards, so the copy installed on disk is signed too - signing only the
finished setup would leave the installed `.exe` unsigned. Tauri restores the
unpatched build output after bundling, which drops the signature from the loose
`target/release/islandwin.exe`, so the script signs that file once more at the
end.

`tauri.conf.json` stays certificate-free; nothing about a particular machine
ends up in version control.

## Choosing the certificate

| Variable | Meaning |
| --- | --- |
| `WINDOWS_CERT_SHA1` | SHA-1 thumbprint. Preferred: unambiguous. |
| `WINDOWS_CERT_SUBJECT` | Subject substring, e.g. `CN=Yassine`. Resolved to a thumbprint. |

With neither set, the single trusted code-signing certificate in the store is
used automatically. Several certificates and no selector is an error rather
than a guess.

Cloud signing providers (Certum SimplySign, SSL.com eSigner) install a virtual
smart card while their agent runs; the certificate then appears in the store
like any other and the same commands work.

A self-signed certificate is never picked automatically. Windows does not trust
its root, so the signature changes no SmartScreen or antivirus verdict; it only
exists to exercise the signing path.

## Local build

```sh
# Certificate thumbprint, or set WINDOWS_CERT_SUBJECT instead.
$env:WINDOWS_CERT_SHA1 = "E67DEFE41D4E1004B3F93808216462F644A9B10F"

npm run package
```

Output:

- `src-tauri/target/release/islandwin.exe`
- `src-tauri/target/release/bundle/nsis/IslandWin_<version>_x64-setup.exe`

`npm run package` prepends the newest Windows SDK `signtool.exe` to `PATH`,
because Tauri calls a bare `signtool.exe` and the SDK is not on `PATH` by
default.

To sign binaries that already exist:

```sh
npm run sign -- src-tauri/target/release/bundle/nsis/IslandWin_0.1.4_x64-setup.exe
```

With no arguments `npm run sign` signs the current release pair. It cannot sign
the application binary packed inside an already-built setup; rebuild with
`npm run package` for that.

## Verify

```sh
npm run verify:signatures
```

The check reports the signer and timestamp for `islandwin.exe` and the setup.
`REQUIRE_SIGNING=1` makes any problem fatal; the release workflow sets it once
a certificate is configured.

A signature must be timestamped. Without a timestamp it stops verifying when
the certificate expires, which is exactly when an old installer still needs to
run.

A self-signed certificate reports `UnknownError` on a machine that does not
trust its root. `ALLOW_SELF_SIGNED=1` accepts that status when the signer
matches `WINDOWS_CERT_SHA1`, which proves the pipeline works without claiming a
trusted release:

```sh
$env:ALLOW_SELF_SIGNED = "1"
npm run verify:signatures
```

Do not set `ALLOW_SELF_SIGNED` in a release. A real release needs a CA-issued
certificate and a `Valid` status.

## CI

The certificate cannot be a repository secret in plain form. Import a base64
PFX into the runner's store first, then run the normal commands:

```yaml
- name: Import signing certificate
  if: ${{ secrets.WINDOWS_CERT_PFX_BASE64 != '' }}
  shell: pwsh
  run: |
    $bytes = [Convert]::FromBase64String("${{ secrets.WINDOWS_CERT_PFX_BASE64 }}")
    $pfx = Join-Path $env:RUNNER_TEMP "signing.pfx"
    [IO.File]::WriteAllBytes($pfx, $bytes)
    $password = ConvertTo-SecureString "${{ secrets.WINDOWS_CERT_PFX_PASSWORD }}" -AsPlainText -Force
    Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\CurrentUser\My -Password $password
    Remove-Item $pfx

- run: npm run package
  env:
    WINDOWS_CERT_SHA1: ${{ secrets.WINDOWS_CERT_SHA1 }}

- run: npm run verify:signatures
  env:
    REQUIRE_SIGNING: "1"
    WINDOWS_CERT_SHA1: ${{ secrets.WINDOWS_CERT_SHA1 }}
```

Remove the imported certificate at the end of the job if the runner is not
ephemeral.

## If antivirus still flags a build

A valid signature raises trust; it does not guarantee a clean scan. Report the
false positive to the vendor with the installer's SHA-256 and the signed file:

```powershell
Get-FileHash src-tauri\target\release\bundle\nsis\IslandWin_0.1.4_x64-setup.exe -Algorithm SHA256
Get-AuthenticodeSignature src-tauri\target\release\bundle\nsis\IslandWin_0.1.4_x64-setup.exe | Format-List
```
