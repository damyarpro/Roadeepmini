# Releasing Roadeep for Windows

A plain `npm run pack` builds an unsigned installer with the updater switched
off, exactly as before. Everything below is opt-in through environment variables
set in the shell that runs the build. Nothing is ever read from a file in the
repository, and no key or certificate belongs in git.

## Offline local AI installer

The Windows installer includes the local brain, speech and speaker assets. Stage all eleven
immutable files listed in `src-tauri/src/local_runtime/assets.rs` from an explicit
trusted asset directory before packaging:

```powershell
node scripts/stage-local-ai.mjs --source 'D:\Roadeep\Roadeep-mini\.codex\local-ai-assets'
npm run pack
```

For another builder, replace the source directory with its local asset cache.
Alternatively set `ROADEEP_LOCAL_ASSET_SOURCE` to that directory before `pack`.
Staging uses bounded streams, checks every exact size/SHA-256 before committing,
and never downloads files. `pack` verifies an already staged bundle when the
source environment variable is absent; missing or corrupt assets stop the build.
`--skip-build` retains the existing behavior for publishing a prebuilt installer.

The generated `local-ai-bundle` contains eleven original assets, `manifest.json`,
source/license notices and a README. Tauri maps it to `local-ai-packages` beside
the installed executable. Large models and engine archives are excluded from git.
First launch prepares the app-owned runtime locally; a present but incomplete
bundle fails visibly instead of fetching replacement model files over the network.
No API credentials are included. API requests still require the configured account.

Run `npm run test:release` for staging and release-script tests. Release verification
must additionally compare all eleven installed package hashes to the native pins and
run the native isolated offline-install harness against the installed package.

## Environment variables

| Variable | Used by | Effect |
|---|---|---|
| `ROADEEP_UPDATE_URL` | app (compile time) | https URL of `latest.json`, e.g. `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`. |
| `ROADEEP_UPDATE_PUBKEY` | app (compile time), pack | Updater public key (one base64 line). Pack also passes it to the CLI, which checks that it matches the private key. |
| `TAURI_SIGNING_PRIVATE_KEY` or `TAURI_SIGNING_PRIVATE_KEY_PATH` | pack | Turns on updater artifacts: the installer's `.sig` and `release/latest.json`. Either variable can hold the key text or the path to the key file. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Tauri CLI | Password of the private key, if it has one. |
| `ROADEEP_RELEASE_BASE_URL` | pack | Required with the private key. The address the release files are downloaded from, e.g. `https://github.com/<owner>/<repo>/releases/download/v{version}`. `{version}` is filled in. |
| `ROADEEP_SIGN_THUMBPRINT` | pack | SHA-1 thumbprint of a code-signing certificate in the Windows certificate store. Signs with `signtool`. |
| `ROADEEP_SIGN_TIMESTAMP_URL` | pack | Timestamp server for the thumbprint mode. Default: `http://timestamp.digicert.com`. |
| `ROADEEP_SIGN_COMMAND` | pack | Any signing command, with `%1` where the file goes (cloud signing, HSM…). Use either this or the thumbprint, not both. |
| `ROADEEP_SIGNTOOL` | pack | Path to `signtool.exe` when it is not in the Windows SDK's usual folder. |

The app only checks for updates when **both** `ROADEEP_UPDATE_URL` and
`ROADEEP_UPDATE_PUBKEY` were set at compile time and the URL is https. Otherwise
Settings → General shows "Automatic updates are not enabled in this build" and
the app never contacts GitHub. If only one of them is set, or the URL is not
https, the updater stays off and the log (`%LOCALAPPDATA%\com.roadeep.desktop\roadeep.log`)
says why.

## One-time setup

### 1. The updater key pair

```powershell
npx tauri signer generate -w "$env:USERPROFILE\.roadeep\updater.key"
```

This writes the private key to `updater.key` and the public key to
`updater.key.pub`, and asks for an optional password.

- Keep the private key **outside the repository**, in a password manager or a
  secrets vault, with a backup.
- **If the private key is lost, installed copies can never update again.** They
  only accept installers signed with it, so users would have to download and
  install a new version by hand.
- The public key is not secret. Put its content in `ROADEEP_UPDATE_PUBKEY`.

### 2. The GitHub repository

Create a **public** repository for the releases. Installed apps download
`latest.json` and the installer without signing in, so a private repository
returns 404 and the app reports that no release was found.

The URL `.../releases/latest/download/latest.json` always points at the release
GitHub marks as **Latest**: publish as a normal release, not as a draft or a
pre-release.

## Making a release

1. Set the version everywhere it lives (package.json, package-lock.json,
   tauri.conf.json, Cargo.toml, Cargo.lock):

   ```powershell
   npm run version -- 0.2.0
   ```

2. Add a `## 0.2.0` section to `CHANGELOG.md` (in `windows/`, or the repository
   root). Its text becomes the release notes shown in Settings. Or pass
   `--notes "…"` or `--notes-file path` to pack (see step 3).

3. Build, in a PowerShell window:

   ```powershell
   $env:ROADEEP_UPDATE_URL = "https://github.com/<owner>/<repo>/releases/latest/download/latest.json"
   $env:ROADEEP_UPDATE_PUBKEY = Get-Content "$env:USERPROFILE\.roadeep\updater.key.pub" -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.roadeep\updater.key"
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "…"   # only if the key has one
   $env:ROADEEP_RELEASE_BASE_URL = "https://github.com/<owner>/<repo>/releases/download/v{version}"
   npm run pack
   ```

   `release/` then holds:

   ```
   Roadeep-Windows-0.2.0-setup.exe       the installer
   Roadeep-Windows-0.2.0-setup.exe.sig   its updater signature
   Roadeep-Windows-setup.exe             the same installer under the rolling name
   latest.json                           what installed apps read
   notes-0.2.0.md                        the release notes
   ```

4. Pack prints the exact publish command (it never runs it), for example:

   ```powershell
   gh release create v0.2.0 release/Roadeep-Windows-0.2.0-setup.exe release/Roadeep-Windows-0.2.0-setup.exe.sig release/latest.json --repo <owner>/<repo> --title "Roadeep 0.2.0" --notes-file release/notes-0.2.0.md
   ```

   The tag must match the one in `ROADEEP_RELEASE_BASE_URL`, since `latest.json`
   points at the installer under that tag.

5. Check: open `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`
   in a browser. It should show the new version.

### What users see

About a minute after launch, and at most once a day (Settings → General →
Updates → "Check automatically", on by default), the app checks `latest.json`.
A new version only shows as a notice; nothing is downloaded or installed until
the user clicks **Install and restart**. The installer is then downloaded,
checked against the public key built into the app (and against the version it
was signed for), and run in passive mode: a progress window, no questions, and
the app restarts on its own.

The update installs over the existing copy without uninstalling it first, so
the user's settings, sign-in and `%LOCALAPPDATA%\com.roadeep.desktop` are kept. The Claude
Code relays in `%LOCALAPPDATA%\com.roadeep.desktop\bin` are refreshed at the next launch; if
a Claude Code session is still using one, the old copy stays until the next
launch.

Upgrading from a build that still used the original project's name (identifier
and folders renamed to `com.roadeep.desktop` / `Roadeep`): the first launch moves
the data folders, the WebView profile and the Credential Manager entries, and
replaces the old autostart entry (`src-tauri/src/migrate.rs`). Claude Code,
Codex and MCP registrations that still run the old relay are never rewritten
automatically: Settings flags them, the usual reviewed install (backup, diff,
confirmation) updates them, and meanwhile the app also answers on the old pipe
names so they keep working. The old relay folders are deleted only from Settings
(`legacy_relay_cleanup`), and only once nothing visible points at them.

## Code signing

Without a certificate, the installer and the app are unsigned: SmartScreen
shows "Windows protected your PC" (More info → Run anyway), and Microsoft
Defender has flagged earlier unsigned builds as `Trojan:Win32/Wacatac.H!ml`, a
machine-learning false positive. Signing is what makes both go away. Updater
signatures (above) are separate and do not replace it.

Options:

- **Certificate in the Windows store** (OV or EV, from a CA, or on a hardware
  token): set `ROADEEP_SIGN_THUMBPRINT`. Pack signs with
  `signtool sign /fd sha256 /sha1 <thumbprint> /tr <timestamp> /td sha256`.
- **Cloud signing** (Azure Trusted Signing, SSL.com eSigner, DigiCert KeyLocker…):
  set `ROADEEP_SIGN_COMMAND` to the provider's command line with `%1` for the
  file, e.g. `trusted-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> %1`.

Either way, pack signs, in this order: the two relays bundled as resources
(`roadeep-hook.exe`, `roadeep-mcp.exe`, which Tauri does not sign), then, through
Tauri, the app itself, the NSIS installer and its uninstaller. To do that it
runs `tauri build --no-bundle`, signs the relays, then `tauri bundle`.

A signed build can also carry updater artifacts: set both groups of variables.

### The self-signed certificate (current setup)

Builds are signed today with a free self-signed certificate, `CN=Roadeep, O=Roadeep`
(thumbprint `1B4B79A94C52771D0218C962465F2D52968E6684`, valid to 2029-10-03). Its
private key is non-exportable and lives only in the build machine's
`Cert:\CurrentUser\My`, so only that machine can sign. Build with:

```
ROADEEP_SIGN_THUMBPRINT=1B4B79A94C52771D0218C962465F2D52968E6684 npm run pack
```

Windows does not know this certificate, so it does **not** remove the
SmartScreen warning on other computers. To make a computer trust it, the
installer offers it (`src-tauri/nsis/hooks.nsh`, public part in
`src-tauri/nsis/Roadeep-code-signing.cer`):

- After an interactive install a dialog asks, in Persian and English, whether
  to trust the publisher "Roadeep". The default answer is No. Yes adds the
  certificate for the current user to **Trusted Root** (Windows asks once more)
  and **Trusted Publishers**, and records that in `HKCU\Software\Roadeep`.
- A silent install (`/S`) never does it, unless started with `/TRUSTCERT`.
- Uninstalling removes the certificate again, but only if the installer added it.

It is an end-entity code-signing certificate (no CA rights): trusting it lets
Windows verify files signed by Roadeep and nothing else. Anyone holding the
private key could sign software those computers trust, so keep the build
machine safe. To trust it by hand instead:

```
Import-Certificate -FilePath .\Roadeep-code-signing.cer -CertStoreLocation Cert:\CurrentUser\Root
Import-Certificate -FilePath .\Roadeep-code-signing.cer -CertStoreLocation Cert:\CurrentUser\TrustedPublisher
```

When the certificate is replaced (a paid OV/EV or Azure Trusted Signing one),
update the thumbprint here and in `hooks.nsh`, and replace the `.cer`, or drop
the trust step entirely: a CA-issued certificate needs none.

### Defender

A brand-new certificate has no reputation yet, so SmartScreen may still warn on
the first releases; that fades as downloads accumulate. If Defender flags a signed build, submit the installer as a false
positive at <https://www.microsoft.com/wdsi/filesubmission>, choosing
"Software developer".
