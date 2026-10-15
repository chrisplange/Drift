# Releasing Drift

How a version gets from a tag to testers, and the one-time setup the in-app updater needs.

## One-time setup: the release signing key

Drift's updater installs a release only if its `SHA256SUMS.txt` is signed with the release
key. The public half is built into the app (`crates/drift-app/src/update/release-key.pub`).
The secret half is a GitHub Actions secret that signs each release. Until the key exists,
releases can't be signed, and the updater offers new versions only as a link to the
release page.

1. Make the key, with the secret half outside the repository:

   ```powershell
   cargo run -p drift-app --example update_key -- $HOME\drift-keys
   ```

   This writes `drift-release.key` (secret) into that folder. It also replaces
   `crates/drift-app/src/update/release-key.pub` with the public key.

2. On GitHub, open **Settings → Secrets and variables → Actions → New repository secret**.
   Name it `MINISIGN_SECRET_KEY` and paste the whole contents of `drift-release.key`, both
   lines.

3. Keep a backup of `drift-release.key` somewhere safe and offline, such as a password
   manager, then delete it from the computer. Anyone who has it can sign an update that
   every copy of Drift would install.

4. Commit `release-key.pub`. Copies of Drift built from then on trust that key.

The key file isn't password-protected, because CI has to use it unattended. The GitHub
secret is what protects it.

**If the key is lost or leaked:** make a new one. Delete `release-key.pub` first, since the
tool won't overwrite it. Then replace the secret and release a new version. Copies built
with the old key can't verify releases signed with the new one, so their testers must
install that version by hand once. After a leak, also delete the old secret straight away.

## Each release

1. Set the version in `Cargo.toml` (`[workspace.package] version`), for example
   `0.1.0-alpha.5`, and merge it to `main`.
2. Tag and push: `git tag v0.1.0-alpha.5`, then `git push origin v0.1.0-alpha.5`.
3. The **Package** workflow builds the Windows installer and zip and the macOS disk image.
   It writes `SHA256SUMS.txt` and signs it into `SHA256SUMS.txt.minisig`, with the trusted
   comment `Drift 0.1.0-alpha.5`. It then checks the signature against the committed
   `release-key.pub`, and stops if that fails. It refuses a tag that doesn't match
   `Cargo.toml`.
4. The files go on a **draft** pre-release. Drafts are invisible to the updater. Look it
   over, edit the notes (the update notice shows them), then **Publish**. From then on,
   copies of Drift offer it.

The updater expects these names on a release. Keep them if the packaging changes:

- `Drift-<version>-windows-x64-setup.exe`
- `Drift-<version>-macos-universal.dmg`
- `SHA256SUMS.txt`
- `SHA256SUMS.txt.minisig`

## Code signing (not set up)

Releases aren't code-signed yet, so Windows SmartScreen and macOS Gatekeeper warn on the
first install. Updates through the app don't warn again. An Authenticode certificate would
remove the Windows warning. An Apple Developer ID with notarisation would remove the macOS
one, and would let the updater also check the new app's signature before swapping it in.
