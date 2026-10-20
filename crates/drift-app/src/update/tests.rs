//! The updater without a network: a fake [`Http`] serves releases signed with a test key.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use semver::Version;
use sha2::{Digest, Sha256};

use super::install::{Install, MAC_SWAP, Plan, WINDOWS_ARGS};
use super::*;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

/// Serves fixed bodies by URL and notes what was asked for.
#[derive(Default)]
struct FakeHttp {
    bodies: HashMap<String, Vec<u8>>,
    asked: RefCell<Vec<String>>,
}

impl FakeHttp {
    fn with(mut self, url: &str, body: impl Into<Vec<u8>>) -> Self {
        self.bodies.insert(url.to_owned(), body.into());
        self
    }
}

impl Http for FakeHttp {
    fn get(
        &self,
        url: &str,
        limit: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, String> {
        self.asked.borrow_mut().push(url.to_owned());
        let body = self.bodies.get(url).cloned().ok_or("no connection")?;
        if body.len() as u64 > limit {
            return Err("too big".into());
        }
        let total = Some(body.len() as u64);
        if !progress(body.len() as u64, total) {
            return Err("cancelled".into());
        }
        Ok(body)
    }
}

/// A release as GitHub's API lists it.
fn api_release(tag: &str, draft: bool, prerelease: bool, assets: &[&str]) -> serde_json::Value {
    let assets: Vec<_> = assets
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "browser_download_url": format!("https://dl.example/{tag}/{name}"),
                "size": 10,
            })
        })
        .collect();
    serde_json::json!({
        "tag_name": tag,
        "name": format!("Drift {}", tag.trim_start_matches('v')),
        "body": "## What's new\n- Things",
        "html_url": format!("https://github.com/chrisplange/drift-releases/releases/tag/{tag}"),
        "draft": draft,
        "prerelease": prerelease,
        "assets": assets,
    })
}

fn list(releases: &[serde_json::Value]) -> Vec<u8> {
    serde_json::to_vec(&releases).unwrap()
}

fn releases(json: &[u8]) -> Vec<Release> {
    parse_releases(json).unwrap()
}

#[test]
fn pre_releases_order_numerically() {
    assert!(v("0.1.0-alpha.10") > v("0.1.0-alpha.9"));
    assert!(v("0.1.0-alpha.9") > v("0.1.0-alpha.4"));
    assert!(v("0.1.0-beta.1") > v("0.1.0-alpha.10"));
    assert!(v("0.1.0") > v("0.1.0-beta.3"));
    assert!(v("0.2.0-alpha.1") > v("0.1.0"));
}

/// A local test build (`scripts/package-windows.ps1` without `-Release`) is labelled
/// `+test.<commit>`: it isn't offered the release it was built from, but is offered the next.
#[test]
fn test_builds_are_offered_only_later_releases() {
    let json = list(&[
        api_release("v0.1.0-beta.1.4", false, true, &[]),
        api_release("v0.1.0-beta.1.5", false, true, &[]),
    ]);
    let all = releases(&json);
    assert_eq!(newest(&all, &v("0.1.0-beta.1.5+test.9490d05")), None);
    let offered = newest(&all, &v("0.1.0-beta.1.4+test.9490d05")).unwrap();
    assert_eq!(offered.version, v("0.1.0-beta.1.5"));
}

#[test]
fn the_running_version_is_semver() {
    assert_eq!(current_version().to_string(), drift_platform::VERSION);
}

#[test]
fn finds_the_newest_published_release() {
    let json = list(&[
        api_release("v0.1.0-alpha.9", false, true, &[]),
        api_release("v0.1.0-alpha.10", false, true, &[]),
        // Drafts never count (GitHub doesn't list them without a login anyway).
        api_release("v0.1.0-alpha.11", true, true, &[]),
        api_release("not-a-version", false, false, &[]),
        api_release("v0.1.0-alpha.3", false, true, &[]),
    ]);
    let found = newest(&releases(&json), &v("0.1.0-alpha.4")).unwrap();
    assert_eq!(found.version, v("0.1.0-alpha.10"));
    assert_eq!(found.title, "Drift 0.1.0-alpha.10");
    assert!(found.notes.contains("What's new"));
    assert!(found.page.ends_with("/tag/v0.1.0-alpha.10"));
}

#[test]
fn nothing_newer() {
    let json = list(&[
        api_release("v0.1.0-alpha.4", false, true, &[]),
        api_release("v0.1.0-alpha.3", false, true, &[]),
    ]);
    assert_eq!(newest(&releases(&json), &v("0.1.0-alpha.4")), None);
    assert_eq!(newest(&[], &v("0.1.0-alpha.4")), None);
    let http = FakeHttp::default().with(RELEASES_URL, json);
    assert_eq!(check(&http, &v("0.1.0-alpha.4")), Ok(None));
}

#[test]
fn a_stable_build_is_offered_only_stable_releases() {
    let json = list(&[
        api_release("v0.2.0-alpha.1", false, true, &[]),
        api_release("v0.1.1", false, false, &[]),
        // Marked as a pre-release on GitHub, though the tag looks stable.
        api_release("v0.1.2", false, true, &[]),
    ]);
    let all = releases(&json);
    assert_eq!(newest(&all, &v("0.1.0")).unwrap().version, v("0.1.1"));
    // An alpha build takes pre-releases too.
    assert_eq!(
        newest(&all, &v("0.1.0-alpha.4")).unwrap().version,
        v("0.2.0-alpha.1")
    );
}

#[test]
fn check_reports_failures() {
    let offline = FakeHttp::default();
    assert!(check(&offline, &v("0.1.0-alpha.4")).is_err());
    let garbled = FakeHttp::default().with(RELEASES_URL, "<html>rate limited</html>");
    assert!(check(&garbled, &v("0.1.0-alpha.4")).is_err());
}

#[test]
fn picks_the_installer_for_each_platform() {
    let names = [
        "Drift-0.1.0-alpha.5-windows-x64.zip",
        "Drift-0.1.0-alpha.5-windows-x64-setup.exe",
        "Drift-0.1.0-alpha.5-macos-universal.dmg",
        "SHA256SUMS.txt",
    ];
    let json = list(&[api_release("v0.1.0-alpha.5", false, true, &names)]);
    let release = &releases(&json)[0];
    let windows = pick_asset(release, Platform::Windows).unwrap();
    assert_eq!(windows.name, "Drift-0.1.0-alpha.5-windows-x64-setup.exe");
    assert_eq!(
        windows.url,
        "https://dl.example/v0.1.0-alpha.5/Drift-0.1.0-alpha.5-windows-x64-setup.exe"
    );
    let mac = pick_asset(release, Platform::MacOs).unwrap();
    assert_eq!(mac.name, "Drift-0.1.0-alpha.5-macos-universal.dmg");

    // An installer for another version on the release isn't taken.
    let json = list(&[api_release(
        "v0.1.0-alpha.6",
        false,
        true,
        &["Drift-0.1.0-alpha.5-windows-x64-setup.exe"],
    )]);
    assert_eq!(pick_asset(&releases(&json)[0], Platform::Windows), None);
}

#[test]
fn reads_checksum_listings() {
    let hex = "ab".repeat(32);
    let sums = format!(
        "{hex}  Drift-1-setup.exe\n{} *Drift-1.dmg\n",
        "cd".repeat(32)
    );
    assert_eq!(listed_sum(&sums, "Drift-1-setup.exe"), Some([0xab; 32]));
    assert_eq!(listed_sum(&sums, "Drift-1.dmg"), Some([0xcd; 32]));
    assert_eq!(listed_sum(&sums, "Drift-1.zip"), None);
    assert_eq!(listed_sum("nothex  Drift-1.dmg", "Drift-1.dmg"), None);
}

/// A release key, and a signed checksum list for `files` at `version`.
struct Signer {
    keys: minisign::KeyPair,
}

impl Signer {
    fn new() -> Self {
        Self {
            keys: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }

    fn public(&self) -> String {
        self.keys.pk.to_box().unwrap().into_string()
    }

    fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.keys.pk),
            &self.keys.sk,
            data,
            Some(comment),
            None,
        )
        .unwrap()
        .into_string()
    }
}

fn sums_for(files: &[(&str, &[u8])]) -> Vec<u8> {
    files
        .iter()
        .map(|(name, bytes)| {
            let hex: String = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            format!("{hex}  {name}\n")
        })
        .collect::<String>()
        .into_bytes()
}

#[test]
fn verifies_signed_checksums() {
    let signer = Signer::new();
    let version = v("0.1.0-alpha.5");
    let installer: &[u8] = b"the installer";
    let sums = sums_for(&[("Drift-setup.exe", installer)]);
    let signature = signer.sign(&sums, &trusted_comment(&version));
    let key = signer.public();

    assert_eq!(verify_sums(&sums, &signature, &key, &version), Ok(()));
    assert_eq!(verify_file(&sums, "Drift-setup.exe", installer), Ok(()));

    // A tampered file.
    assert_eq!(
        verify_file(&sums, "Drift-setup.exe", b"the installer!"),
        Err(VerifyError::Mismatch("Drift-setup.exe".into()))
    );
    assert_eq!(
        verify_file(&sums, "Drift-other.exe", installer),
        Err(VerifyError::NotListed("Drift-other.exe".into()))
    );
    // A tampered checksum list.
    let mut forged = sums.clone();
    forged[0] = if forged[0] == b'0' { b'1' } else { b'0' };
    assert_eq!(
        verify_sums(&forged, &signature, &key, &version),
        Err(VerifyError::BadSignature)
    );
    // Signed with another key.
    let other = Signer::new();
    assert_eq!(
        verify_sums(&sums, &signature, &other.public(), &version),
        Err(VerifyError::BadSignature)
    );
    // An old release's signed list put on a newer release.
    assert_eq!(
        verify_sums(&sums, &signature, &key, &v("0.1.0-alpha.6")),
        Err(VerifyError::WrongVersion("Drift 0.1.0-alpha.5".into()))
    );
    // Garbage signature; no key in this build.
    assert_eq!(
        verify_sums(&sums, "not a signature", &key, &version),
        Err(VerifyError::BadSignatureFile)
    );
    assert_eq!(
        verify_sums(
            &sums,
            &signature,
            "untrusted comment: no key yet\n",
            &version
        ),
        Err(VerifyError::NoKey)
    );
}

#[test]
fn the_bundled_key_is_a_key_or_the_placeholder() {
    let key = RELEASE_KEY.trim();
    let usable = minisign_verify::PublicKey::decode(key).is_ok();
    assert!(
        usable || key.lines().count() == 1,
        "release-key.pub is malformed"
    );
}

/// A fresh, empty folder for a test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drift-update-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A release with an installer, its checksums and signature, served by a fake network.
fn served_release(signer: &Signer, installer: &[u8], signed_for: &str) -> (Release, FakeHttp) {
    let version = v("0.1.0-alpha.5");
    let name = Platform::Windows.asset_name(&version);
    let json = list(&[api_release(
        "v0.1.0-alpha.5",
        false,
        true,
        &[&name, SUMS_NAME, SIGNATURE_NAME],
    )]);
    let release = releases(&json).remove(0);
    let sums = sums_for(&[(&name, b"the real installer")]);
    let signature = signer.sign(&sums, signed_for);
    let http = FakeHttp::default()
        .with(&release.asset(&name).unwrap().url, installer)
        .with(&release.asset(SUMS_NAME).unwrap().url, sums)
        .with(&release.asset(SIGNATURE_NAME).unwrap().url, signature);
    (release, http)
}

#[test]
fn downloads_and_verifies_an_installer() {
    let signer = Signer::new();
    let (release, http) = served_release(&signer, b"the real installer", "Drift 0.1.0-alpha.5");
    let dir = scratch("good");
    let mut seen = 0;
    let path = download(
        &http,
        &release,
        Platform::Windows,
        &signer.public(),
        &dir,
        &mut |got, _| {
            seen = got;
            true
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"the real installer");
    assert_eq!(seen, 18);

    // A second time, the verified file already there is used.
    let asked = http.asked.borrow().len();
    download(
        &http,
        &release,
        Platform::Windows,
        &signer.public(),
        &dir,
        &mut |_, _| true,
    )
    .unwrap();
    assert_eq!(
        http.asked.borrow().len(),
        asked + 2,
        "only the small files again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_a_tampered_download() {
    let signer = Signer::new();
    let (release, http) = served_release(&signer, b"a swapped installer", "Drift 0.1.0-alpha.5");
    let dir = scratch("tampered");
    let err = download(
        &http,
        &release,
        Platform::Windows,
        &signer.public(),
        &dir,
        &mut |_, _| true,
    )
    .unwrap_err();
    assert!(err.contains("doesn't match"), "{err}");
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "nothing kept"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_a_release_signed_with_another_key_before_downloading_it() {
    let signer = Signer::new();
    let (release, http) = served_release(&signer, b"the real installer", "Drift 0.1.0-alpha.5");
    let dir = scratch("other-key");
    let err = download(
        &http,
        &release,
        Platform::Windows,
        &Signer::new().public(),
        &dir,
        &mut |_, _| true,
    )
    .unwrap_err();
    assert!(err.contains("isn't signed"), "{err}");
    let installer = release.asset(&Platform::Windows.asset_name(&release.version));
    assert!(!http.asked.borrow().contains(&installer.unwrap().url));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_release_without_a_signature_is_not_installed() {
    let json = list(&[api_release(
        "v0.1.0-alpha.5",
        false,
        true,
        &["Drift-0.1.0-alpha.5-windows-x64-setup.exe", SUMS_NAME],
    )]);
    let release = releases(&json).remove(0);
    let err = download(
        &FakeHttp::default(),
        &release,
        Platform::Windows,
        "",
        &scratch("unsigned"),
        &mut |_, _| true,
    )
    .unwrap_err();
    assert!(err.contains("signature"), "{err}");
}

#[test]
fn knows_how_it_was_installed() {
    // Windows: the installer leaves an uninstaller beside the program.
    let dir = scratch("installed");
    let exe = dir.join("drift.exe");
    assert_eq!(
        Install::from_exe(&exe, Some(Platform::Windows)),
        Install::Manual
    );
    std::fs::write(dir.join("unins000.exe"), b"").unwrap();
    assert_eq!(
        Install::from_exe(&exe, Some(Platform::Windows)),
        Install::Installer
    );
    let _ = std::fs::remove_dir_all(&dir);

    // macOS: the program is inside an app bundle.
    let bundle = PathBuf::from("/Applications/Drift.app");
    let exe = bundle.join("Contents").join("MacOS").join("drift");
    assert_eq!(
        Install::from_exe(&exe, Some(Platform::MacOs)),
        Install::Bundle(bundle)
    );
    let built = PathBuf::from("/Users/me/Drift/target/release/drift");
    assert_eq!(
        Install::from_exe(&built, Some(Platform::MacOs)),
        Install::Manual
    );
    assert_eq!(Install::from_exe(&built, None), Install::Manual);
}

#[test]
fn windows_installs_silently_and_reopens_drift() {
    assert!(WINDOWS_ARGS.contains(&"/SILENT"));
    assert!(WINDOWS_ARGS.contains(&"/CLOSEAPPLICATIONS"));
    assert!(WINDOWS_ARGS.contains(&"/RELAUNCH=1"));
    let iss = include_str!("../../../../installer/windows/drift.iss");
    assert!(
        iss.to_ascii_lowercase().contains("{param:relaunch|0}"),
        "drift.iss reads /RELAUNCH"
    );
    let plan = install::prepare(&PathBuf::from("setup.exe"), &Install::Installer).unwrap();
    assert_eq!(
        plan,
        Plan::Windows {
            installer: "setup.exe".into()
        }
    );
    assert!(install::prepare(&PathBuf::from("setup.exe"), &Install::Manual).is_err());
}

#[test]
fn the_mac_swap_waits_and_puts_the_old_app_back_on_failure() {
    assert!(MAC_SWAP.contains(r#"kill -0 "$pid""#));
    assert!(MAC_SWAP.contains(r#"else mv "$old" "$app""#));
    assert!(MAC_SWAP.contains("com.apple.quarantine"));
    assert!(MAC_SWAP.trim_end().ends_with(r#"open "$app""#));
}
