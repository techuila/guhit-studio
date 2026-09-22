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

The workflow signs each update artifact with a minisign private key. Without it
the build fails, because `bundle.createUpdaterArtifacts` is on.

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

It also uploads `latest.json`, which lists the version, the release notes, and
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

## The app is not code signed yet

There is no Apple Developer ID and no Windows code signing certificate. That is
a separate purchase and a separate set of secrets. Until then:

**macOS.** The first install is the awkward one. Gatekeeper refuses to open a
downloaded app that has no Developer ID, and the user has to right-click the app
and choose Open, or allow it once under System Settings -> Privacy & Security.
Updates are easier: the updater replaces the bundle of an app the user already
opened, so Gatekeeper does not ask again. Apple's quarantine flag is not set on
the files the updater writes. Note also DECISIONS D13: an unsigned build gets a
new code identity each time, which is why the Claude API key lives in a private
file instead of the keychain.

**Windows.** SmartScreen shows "Windows protected your PC" on the downloaded
installer until the certificate builds reputation, which an unsigned build never
does. The user clicks "More info" then "Run anyway". The in-app update runs the
NSIS installer silently from a process the user already trusts, so this warning
is mostly a first-install problem too.

Neither of these is faked or worked around anywhere in the build. When a
Developer ID and a Windows certificate exist, they are added as their own
secrets and the bundle config gains a `macOS.signingIdentity` and a
`windows.certificateThumbprint`; the updater key is unrelated and does not
change.

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
