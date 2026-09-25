# Releasing

Guhit Studio updates itself. An installed copy asks GitHub for the newest
release three seconds after launch and every six hours after that, and offers
"Update and restart" when there is one. Publishing a release is one tag push.

## Cut a release

```bash
node scripts/bump-version.mjs 0.1.1   # sets the version in all three files
cargo check -p guhit-studio           # refreshes Cargo.lock with the new version
git add -A
git commit -m "chore: release v0.1.1"
git push
git tag v0.1.1
git push origin v0.1.1
```

The tag starts `.github/workflows/release.yml`. Nothing else does: `build.yml`
runs on branch pushes and pull requests, so a tag builds exactly once.

The version lives in three files and `scripts/bump-version.mjs` sets all three
from one argument:

| File | Why |
|---|---|
| `src-tauri/tauri.conf.json` | Source of truth. The updater compares this against `latest.json`. |
| `package.json` | Keeps the npm package in step. |
| `Cargo.toml` (`[workspace.package]`) | Version of every crate in the workspace. |

The tag must be `v` plus that same version, for example `v0.1.1`. A tag that
does not match the config version produces a release whose `latest.json` says
something different from the installers in it.

## The secret to add

The workflow signs each update artifact with a minisign private key. Without
the secret the release still builds and ships every installer, it just leaves
out the update files (`latest.json` and the `.sig` files) and the run shows a
warning. Installed copies then keep working but are not offered that version
as an update, so add the secret before the release you want them to receive.

- Secret name: **`TAURI_SIGNING_PRIVATE_KEY`** (repository secret, Settings ->
  Secrets and variables -> Actions -> New repository secret).
- Value: the whole contents of `~/.tauri/guhit-studio.key` on Axl's machine.
- The key was generated with `--ci`, so it has **no password**. The workflow
  still sets `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to an empty string, which is
  what the CLI expects. No second secret is needed.
- The matching public key is already in `src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`, and the file is at
  `~/.tauri/guhit-studio.key.pub`.

**Back the private key up somewhere safe and offline.** Every installed copy of
Guhit Studio only trusts updates signed by this one key. If it is lost, no
release can ever reach the people who already installed the app: they would
each have to download and install a new build by hand. If it leaks, someone
else can sign an update that those copies will install, so treat it like a
signing certificate. Never commit it, never paste it into a chat or an issue.

## What the workflow produces

`release.yml` builds three ways and publishes one GitHub release named
`Guhit Studio v0.1.1`, not a draft and not a prerelease:

| Runner | Target | Ships |
|---|---|---|
| macos-latest | `aarch64-apple-darwin` | `.dmg`, `.app.tar.gz` + `.app.tar.gz.sig` |
| macos-latest | `x86_64-apple-darwin` | `.dmg`, `.app.tar.gz` + `.app.tar.gz.sig` |
| windows-latest | x64 | `.msi`, NSIS `-setup.exe` + `.sig` |

Each job also uploads its installer a second time under a fixed name. The
website's download buttons link to these through the permanent
`releases/latest/download/` URL, so a download starts at once, without the
GitHub page, and the links never change between versions:

| File | For |
|---|---|
| `Guhit-Studio-mac-apple-silicon.dmg` | Macs with Apple silicon (the main macOS button) |
| `Guhit-Studio-mac-intel.dmg` | Macs with an Intel chip (the link under the buttons) |
| `Guhit-Studio-windows-setup.exe` | Windows 10 and 11 |

```
https://github.com/techuila/guhit-studio/releases/latest/download/Guhit-Studio-mac-apple-silicon.dmg
```

With the updater key set, it also uploads `latest.json`, which lists the version, the release notes, and
one signed download per platform key (`darwin-aarch64`, `darwin-x86_64`,
`windows-x86_64`). That file is what the app reads, through the permanent URL

```
https://github.com/techuila/guhit-studio/releases/latest/download/latest.json
```

The repository is public, so no token is involved at update time.

macOS updates use the `.app.tar.gz`, because that is the format the updater can
unpack over a running app. Windows updates use the NSIS `setup.exe`
(`updaterJsonPreferNsis: true`); an MSI cannot cleanly replace an installation
that is running. The `.dmg` and `.msi` are there for first-time installs.

## Test an update end to end

1. Publish version A the normal way (`0.1.1`, tag `v0.1.1`).
2. Install A from its `.dmg` or `setup.exe` and open it once.
3. Bump to B (`0.1.2`), tag and push. Wait for the release to appear.
4. Open the installed A. Within a few seconds the notice reads
   "Guhit Studio 0.1.2 is available". Press "Update and restart".
5. It should download with a progress bar, relaunch on its own, and the About
   or the tag in the release notes should now show B.

To force a check without waiting, open the command palette and run
"Check for updates". When nothing is newer it says "You are up to date".

Local builds are not updatable in a useful way: a `pnpm tauri dev` or an
unpacked `target/release` build has whatever version is in the config, and the
update it downloads would replace the installed app, not the build tree. Always
test against a real install.

## Code signing

**macOS: Developer ID and notarization.** With the Apple secrets below, the
release signs each Mac app with the Developer ID Application certificate and
has Apple notarize it, and Tauri staples the ticket to the app. A downloaded
copy then opens with macOS's normal "downloaded from the internet" question
and nothing else. Without the secrets the app is ad-hoc signed
(`bundle.macOS.signingIdentity` is `"-"`, which local builds keep) and macOS
blocks the first open with "Apple could not verify Guhit Studio is free of
malware". The run shows a warning when a secret is missing.

Ad-hoc signing is still better than none: with only the linker's signature on
the binary, the bundle's resources were unsealed and a downloaded copy on Apple
silicon was reported as "damaged" with no way to open it.

The secrets (Settings -> Secrets and variables -> Actions, or `gh secret set`).
They have the same names as in Axl's other Mac apps (TopNotch), which use the
same certificate:

| Secret | Value |
|---|---|
| `MACOS_CERT_P12` | The Developer ID Application certificate and its private key, exported from Keychain Access as a `.p12`, in base64 |
| `MACOS_CERT_PASSWORD` | The password chosen when exporting the `.p12` |
| `APPLE_ID` | The Apple Account email of the developer account |
| `APPLE_APP_PASSWORD` | An app-specific password for that account, not the account password |
| `APPLE_TEAM_ID` | The team ID, the code in brackets after the certificate's name |
| `MACOS_SIGN_IDENTITY` | Optional. The full identity name; without it the workflow matches `Developer ID Application` |

The certificate must say **Developer ID Application**. Apple Development,
iPhone Distribution and Apple Distribution certificates cannot sign an app that
is notarized for download outside the App Store. Create it with the **G2
Sub-CA**: a certificate from the Previous Sub-CA can never outlive that CA,
which ends on 2027-02-01. Check a certificate with
`security find-certificate -c "Developer ID Application" -p | openssl x509 -noout -issuer -enddate`
(`OU=G2` in the issuer means G2). Only the team's Account Holder can create one.
Before it expires, make a new one and replace the two certificate secrets;
builds already shipped keep opening.

Setting them up, on the Mac that holds the certificate:

1. In Keychain Access, under the login keychain's My Certificates, expand
   "Developer ID Application: <name> (<team ID>)" so its private key shows,
   select both, choose File, Export Items, save as a `.p12` and set a
   password.
2. At account.apple.com, Sign-In and Security, App-Specific Passwords, create
   one named for the release.
3. Add the secrets. Each `gh secret set` without `--body` asks for the value
   without showing it:

```bash
base64 -i DeveloperID.p12 | gh secret set MACOS_CERT_P12 -R techuila/guhit-studio
gh secret set MACOS_CERT_PASSWORD -R techuila/guhit-studio
gh secret set APPLE_ID -R techuila/guhit-studio
gh secret set APPLE_APP_PASSWORD -R techuila/guhit-studio
gh secret set APPLE_TEAM_ID -R techuila/guhit-studio
```

4. Keep the `.p12` in one safe place (the password protects it; keep the
   password in a password manager, not next to the file). Every Mac app that
   uses this certificate needs the same file, and Apple cannot re-issue the
   private key.

How the workflow uses them: a step maps them to the names Tauri reads
(`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`,
`APPLE_PASSWORD`, `APPLE_TEAM_ID`) and puts each one into the environment only
when it has a value, because Tauri treats a variable that is set but empty as
present and would try to import an empty certificate. It sets
`APPLE_SIGNING_IDENTITY` to `MACOS_SIGN_IDENTITY`, or else to
`Developer ID Application`, which overrides the
`"-"` in the config; Tauri only checks that the imported certificate's name
contains it. Tauri imports the certificate into a temporary keychain, signs
the app with the hardened runtime, notarizes it with `notarytool`, staples it,
and signs the DMG. It does not notarize the DMG, and a downloaded DMG is
checked on its own, so the next step notarizes and staples the DMG, runs
`spctl` on the DMG and the app the way a fresh Mac would, and replaces the
DMG tauri-action uploaded. Both Mac builds go through this separately, which
adds a few minutes to each. The Claude API key stays in its private file
(D13), so signing needs no keychain entitlement.

Check a downloaded build:

```bash
spctl -a -vvv -t exec "/Applications/Guhit Studio.app"
xcrun stapler validate "/Applications/Guhit Studio.app"
```

The first should say `source=Notarized Developer ID`.

**Opening a copy that is not notarized.** The first open is refused. The user
clicks Done, then System Settings -> Privacy & Security -> Open Anyway (macOS
15 and newer; the button shows for about an hour after the refused open), or
right-clicks the app and chooses Open (macOS 14 and older). When the button
does not show, Terminal clears the download flag:
`xattr -dr com.apple.quarantine "/Applications/Guhit Studio.app"`.
Updates are easier: the updater replaces the bundle of an app the user already
opened, and Apple's quarantine flag is not set on the files it writes. Note
also DECISIONS D13: an ad-hoc signed build gets a new code identity each time,
which is why the Claude API key lives in a private file instead of the
keychain.

**Windows.** There is no code signing certificate yet. SmartScreen shows
"Windows protected your PC" on the downloaded installer until the certificate
builds reputation, which an unsigned build never does. The user clicks "More
info" then "Run anyway". The in-app update runs the NSIS installer silently
from a process the user already trusts, so this warning is mostly a
first-install problem too. When a certificate exists, the bundle config gains a
`windows.certificateThumbprint` and the workflow its own secret.

Neither platform's warning is faked or worked around anywhere in the build. The
updater key is unrelated to code signing and does not change.

## Rotating the updater key

Only if the private key leaks or is lost.

```bash
pnpm tauri signer generate --ci -w ~/.tauri/guhit-studio.key
```

Put the new `.key.pub` contents into `plugins.updater.pubkey`, replace the
`TAURI_SIGNING_PRIVATE_KEY` secret, and release.

**Everyone who already installed the app has to download and install it again
by hand.** Their copy only trusts the old key, so it will reject every update
signed with the new one, and it will not tell them why in any useful way. Say so
in the release notes and anywhere the app is announced. This is the reason the
private key is worth backing up properly.
