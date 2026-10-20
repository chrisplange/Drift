//! In-app updates (native only): find a newer published release on GitHub, download the
//! installer for this platform, and check it before anything runs.
//!
//! A release is trusted only if its `SHA256SUMS.txt` carries a minisign signature made with
//! the release key (its public half is compiled in, [`RELEASE_KEY`]) and names the version,
//! and the download matches its checksum there. GitHub alone is not the root of trust.
//!
//! The network sits behind [`Http`], so everything here can be tested without it.

pub(crate) mod install;
pub(crate) mod net;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Published releases, newest first, from the public releases repository (it has no source).
/// Drafts are never listed for a client that isn't signed in, so only releases that have been
/// published are seen. Keep in step with `RELEASES_REPO` in .github/workflows/package.yml.
pub(crate) const RELEASES_URL: &str =
    "https://api.github.com/repos/chrisplange/drift-releases/releases?per_page=30";
/// Where to download Drift by hand: the website's download page.
pub(crate) const DOWNLOAD_PAGE: &str = crate::links::DOWNLOAD;
/// The public half of the key that signs releases (see docs/releasing.md).
pub(crate) const RELEASE_KEY: &str = include_str!("release-key.pub");
/// The checksum list on each release, and its signature.
pub(crate) const SUMS_NAME: &str = "SHA256SUMS.txt";
pub(crate) const SIGNATURE_NAME: &str = "SHA256SUMS.txt.minisig";

// Limits on what is read from the network.
const LIST_LIMIT: u64 = 8 << 20;
const SUMS_LIMIT: u64 = 64 << 10;
const SIGNATURE_LIMIT: u64 = 4 << 10;
const ASSET_LIMIT: u64 = 1 << 30;

/// The running version.
pub(crate) fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// Fetches a URL. The real one is [`net::Agent`]; tests use a fake.
pub(crate) trait Http {
    /// The body of `url`, at most `limit` bytes. `progress` hears the bytes read so far and
    /// the length, if known, and returns false to stop.
    fn get(
        &self,
        url: &str,
        limit: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, String>;
}

/// A downloadable file on a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    #[serde(default)]
    pub size: u64,
}

/// A published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Release {
    pub version: Version,
    pub title: String,
    /// The release notes (Markdown).
    pub notes: String,
    /// The release's page on GitHub.
    pub page: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

impl Release {
    pub(crate) fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The releases in a GitHub API response: drafts, and tags that aren't versions, are left out.
pub(crate) fn parse_releases(json: &[u8]) -> Result<Vec<Release>, String> {
    let list: Vec<ApiRelease> =
        serde_json::from_slice(json).map_err(|e| format!("unexpected reply from GitHub: {e}"))?;
    Ok(list
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version =
                Version::parse(r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name)).ok()?;
            Some(Release {
                title: r
                    .name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("Drift {version}")),
                version,
                notes: r.body.unwrap_or_default(),
                page: r.html_url,
                prerelease: r.prerelease,
                assets: r.assets,
            })
        })
        .collect())
}

/// The newest release above `current`. While `current` is a pre-release (alpha, beta), other
/// pre-releases count; a stable build is only offered stable releases.
pub(crate) fn newest(releases: &[Release], current: &Version) -> Option<Release> {
    let wants_pre = !current.pre.is_empty();
    releases
        .iter()
        .filter(|r| r.version > *current)
        .filter(|r| wants_pre || (r.version.pre.is_empty() && !r.prerelease))
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned()
}

/// Ask GitHub for the newest release above `current`; `None` when this is the newest.
pub(crate) fn check(http: &dyn Http, current: &Version) -> Result<Option<Release>, String> {
    let json = http.get(RELEASES_URL, LIST_LIMIT, &mut |_, _| true)?;
    Ok(newest(&parse_releases(&json)?, current))
}

/// The systems Drift has an installer for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    Windows,
    MacOs,
}

impl Platform {
    pub(crate) fn current() -> Option<Self> {
        if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else {
            None
        }
    }

    /// The installer's file name on a release (as `scripts/package-*` name them).
    pub(crate) fn asset_name(self, version: &Version) -> String {
        match self {
            Self::Windows => format!("Drift-{version}-windows-x64-setup.exe"),
            Self::MacOs => format!("Drift-{version}-macos-universal.dmg"),
        }
    }
}

/// The installer for `platform` on `release`, if it has one.
pub(crate) fn pick_asset(release: &Release, platform: Platform) -> Option<&Asset> {
    release.asset(&platform.asset_name(&release.version))
}

/// Why a download isn't trusted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VerifyError {
    /// This build has no usable release key.
    NoKey,
    /// The signature file can't be read.
    BadSignatureFile,
    /// The checksum list isn't signed by the release key.
    BadSignature,
    /// Signed, but for another version (an old list put on a new release).
    WrongVersion(String),
    /// The file isn't in the checksum list.
    NotListed(String),
    /// The file doesn't match its checksum.
    Mismatch(String),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoKey => write!(f, "this build of Drift has no release key to check updates"),
            Self::BadSignatureFile => write!(f, "the release's signature can't be read"),
            Self::BadSignature => write!(f, "the release isn't signed by Drift's release key"),
            Self::WrongVersion(c) => write!(f, "the release's signature is for {c}"),
            Self::NotListed(n) => write!(f, "{n} isn't in the release's checksums"),
            Self::Mismatch(n) => write!(f, "{n} doesn't match its checksum"),
        }
    }
}

/// The signed comment a release's checksum list must carry.
pub(crate) fn trusted_comment(version: &Version) -> String {
    format!("Drift {version}")
}

/// Check that `sums` (a `sha256sum` listing) is signed with `key` for `version`.
pub(crate) fn verify_sums(
    sums: &[u8],
    signature: &str,
    key: &str,
    version: &Version,
) -> Result<(), VerifyError> {
    let key = minisign_verify::PublicKey::decode(key.trim())
        .or_else(|_| minisign_verify::PublicKey::from_base64(key.trim()))
        .map_err(|_| VerifyError::NoKey)?;
    let signature =
        minisign_verify::Signature::decode(signature).map_err(|_| VerifyError::BadSignatureFile)?;
    key.verify(sums, &signature, false)
        .map_err(|_| VerifyError::BadSignature)?;
    let comment = signature.trusted_comment().trim();
    if comment != trusted_comment(version) {
        return Err(VerifyError::WrongVersion(comment.to_owned()));
    }
    Ok(())
}

/// The SHA-256 listed for `name` in a `sha256sum` listing (`<hex>  <name>` or `<hex> *<name>`).
pub(crate) fn listed_sum(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|line| {
        let (hex, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start();
        let file = file.strip_prefix('*').unwrap_or(file);
        (file == name).then(|| decode_hex(hex)).flatten()
    })
}

fn decode_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Check a downloaded file against a verified checksum list.
pub(crate) fn verify_file(sums: &[u8], name: &str, bytes: &[u8]) -> Result<(), VerifyError> {
    let sums = String::from_utf8_lossy(sums);
    let want = listed_sum(&sums, name).ok_or_else(|| VerifyError::NotListed(name.to_owned()))?;
    if Sha256::digest(bytes).as_slice() != want {
        return Err(VerifyError::Mismatch(name.to_owned()));
    }
    Ok(())
}

/// Where downloaded updates are kept until they're installed.
pub(crate) fn download_dir(version: &Version) -> PathBuf {
    std::env::temp_dir()
        .join("Drift updates")
        .join(version.to_string())
}

/// Download the installer for `platform` from `release` into `dir` and verify it with `key`.
/// A file already there that passes is used again. `progress` as for [`Http::get`].
pub(crate) fn download(
    http: &dyn Http,
    release: &Release,
    platform: Platform,
    key: &str,
    dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<PathBuf, String> {
    let asset = pick_asset(release, platform)
        .ok_or_else(|| format!("Drift {} has no download for this system", release.version))?;
    let missing = |name: &str| format!("Drift {} has no {name}", release.version);
    let sums_asset = release.asset(SUMS_NAME).ok_or_else(|| missing(SUMS_NAME))?;
    let sig_asset = release
        .asset(SIGNATURE_NAME)
        .ok_or_else(|| missing("signature"))?;
    let sums = http.get(&sums_asset.url, SUMS_LIMIT, &mut |_, _| true)?;
    let signature = http.get(&sig_asset.url, SIGNATURE_LIMIT, &mut |_, _| true)?;
    // Check the signature before fetching anything large.
    verify_sums(
        &sums,
        &String::from_utf8_lossy(&signature),
        key,
        &release.version,
    )
    .map_err(|e| e.to_string())?;

    let path = dir.join(&asset.name);
    if let Ok(bytes) = std::fs::read(&path)
        && verify_file(&sums, &asset.name, &bytes).is_ok()
    {
        return Ok(path);
    }
    let bytes = http.get(&asset.url, ASSET_LIMIT, progress)?;
    verify_file(&sums, &asset.name, &bytes).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("could not keep the download: {e}"))?;
    let part = dir.join(format!("{}.part", asset.name));
    std::fs::write(&part, &bytes)
        .and_then(|()| std::fs::rename(&part, &path))
        .map_err(|e| format!("could not keep the download: {e}"))?;
    Ok(path)
}
