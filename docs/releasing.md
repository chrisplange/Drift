# Releasing Drift

How a version gets from a tag to testers, and the one-time setup the in-app updater needs.

Releases are published in the public **releases repository**, `chrisplange/drift-releases`.
It holds the downloads, the release notes and the public issues, and no source. This
repository builds and signs each release and drafts it there. The in-app updater
(`RELEASES_URL` in `crates/drift-app/src/update/mod.rs`) and the website
(`repo` in `website/src/site.config.mjs`) read it. Keep the three names in step with
`RELEASES_REPO` in `.github/workflows/package.yml`.

## One-time setup: the token for the releases repository

The **Package** workflow drafts releases in the releases repository with a fine-grained
token that can write to that repository and nothing else.

1. On GitHub, open your **Settings → Developer settings → Personal access tokens →
   Fine-grained tokens → Generate new token**.
2. Name it, for example `Drift releases`. Set an expiry (a year at most) and note the date:
   releases fail with "the RELEASES_TOKEN secret is missing" or an authentication error
   once it lapses.
3. **Resource owner:** you. **Repository access:** *Only select repositories*, then pick
   `drift-releases` alone.
4. **Permissions → Repository permissions → Contents: Read and write.** Leave everything
   else at *No access* (Metadata: Read-only is added by itself).
5. Generate it and copy it. In this repository, open **Settings → Secrets and variables →
   Actions → New repository secret**, name it `RELEASES_TOKEN`, and paste it. Don't keep
   another copy: make a new token if you ever need it again.

To renew it, generate a new token the same way and replace the secret's value.

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

## Third-party notices

`THIRD-PARTY-NOTICES.txt` lists the licences and copyright notices of the fonts and crates
built into Drift. Their licences ask for it to go with every copy, so the packaging scripts
put it in the Windows installer and zip and in the macOS app's `Contents/Resources`, the web
build serves it next to the app, and **About → Licences…** shows it in every build.

It is made from `Cargo.lock`, so it changes whenever a dependency does. The CI check
**Third-party notices** fails until it is regenerated:

```powershell
cargo install cargo-about --locked --features cli
python scripts/notices.py
```

Commit the new file with the dependency change. `about.toml` says which platforms and
licences it covers; a dependency under a licence not listed there (or in `deny.toml`) needs
a look before it goes in.

## Each release

1. Set the version, for example `0.1.0-beta.1.5`, in `Cargo.toml`
   (`[workspace.package] version`), `README.md`, `docs/guide/README.md` and the example in
   `.github/ISSUE_TEMPLATE/bug.yml`, and run `cargo update -w` for `Cargo.lock`. Merge it to
   `main`. The CI checks must pass, including **Third-party notices** (see above).
2. Tag `main` and push: `git tag v0.1.0-beta.1.5 origin/main`, then
   `git push origin v0.1.0-beta.1.5`.
3. The **Package** workflow builds the Windows installer and zip and the macOS disk image.
   It writes `SHA256SUMS.txt` and signs it into `SHA256SUMS.txt.minisig`, with the trusted
   comment `Drift 0.1.0-beta.1.5`. It then checks the signature against the committed
   `release-key.pub`, and stops if that fails. It refuses a tag that doesn't match
   `Cargo.toml`.
4. The files go on a **draft** pre-release in the releases repository. Its notes list the
   pull requests merged since the previous tag, by title, without links (this repository
   isn't public). Drafts are invisible to the updater. Look it over and edit the notes (the
   update notice shows them). While this repository is still public, the same draft is also
   made here, for copies of Drift up to Beta 1.4, which look for updates here.
5. Publish it (and the copy here, if there is one):

   ```powershell
   gh release edit v0.1.0-beta.1.5 --repo chrisplange/drift-releases --draft=false --prerelease
   ```

   From then on, copies of Drift offer it.
6. Rebuild the website so its download links and release notes catch up:

   ```powershell
   gh workflow run website.yml --ref main
   ```

The updater expects these names on a release. Keep them if the packaging changes:

- `Drift-<version>-windows-x64-setup.exe`
- `Drift-<version>-macos-universal.dmg`
- `SHA256SUMS.txt`
- `SHA256SUMS.txt.minisig`

Each release also carries `drift_profiles-<version>.zip`, the Blender add-on that reads the
profiles files Drift writes (see the guide's
[Profiles in Blender](guide/tutorials/13-profiles-in-blender.md)). The Package workflow
builds it with `python scripts/package-blender-addon.py`, which stamps Drift's version into
its manifest. CI fails if the version in
`integrations/blender/drift_profiles/blender_manifest.toml` isn't Cargo.toml's. After a
version bump, run the script once and commit the manifest.

## Code signing (not set up)

Releases aren't code-signed yet, so Windows SmartScreen and macOS Gatekeeper warn on the
first install. Updates through the app don't warn again. An Authenticode certificate would
remove the Windows warning. An Apple Developer ID with notarisation would remove the macOS
one, and would let the updater also check the new app's signature before swapping it in.
