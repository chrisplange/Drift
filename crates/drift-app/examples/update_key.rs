//! Make Drift's release signing key (once). See docs/releasing.md.
//!
//!     cargo run -p drift-app --example update_key -- <folder outside the repository>
//!
//! Writes the secret key to `<folder>/drift-release.key` (for the `MINISIGN_SECRET_KEY`
//! GitHub Actions secret, and a safe backup), and the public key into the app at
//! `crates/drift-app/src/update/release-key.pub`. Never overwrites a key.

use std::path::{Path, PathBuf};

fn main() -> Result<(), String> {
    let folder = std::env::args_os().nth(1).map(PathBuf::from).ok_or(
        "usage: cargo run -p drift-app --example update_key -- <folder for the secret key>",
    )?;
    let secret_path = folder.join("drift-release.key");
    let public_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/update/release-key.pub");
    if secret_path.exists() {
        return Err(format!("{} already exists", secret_path.display()));
    }
    let placeholder = std::fs::read_to_string(&public_path)
        .map(|text| text.trim().lines().count() < 2)
        .unwrap_or(true);
    if !placeholder {
        return Err(format!(
            "{} already holds a key: delete it first if you really mean to replace it \
             (builds with the old key can then no longer update)",
            public_path.display()
        ));
    }
    let keys = minisign::KeyPair::generate_unencrypted_keypair().map_err(|e| e.to_string())?;
    let secret = keys
        .sk
        .to_box(Some("Drift release key"))
        .map_err(|e| e.to_string())?
        .into_string();
    let public = keys.pk.to_box().map_err(|e| e.to_string())?.into_string();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(&secret_path, secret).map_err(|e| e.to_string())?;
    std::fs::write(&public_path, public).map_err(|e| e.to_string())?;
    println!("secret key: {}", secret_path.display());
    println!("public key: {}", public_path.display());
    println!();
    println!("Next: add the secret key's contents as the MINISIGN_SECRET_KEY secret on GitHub,");
    println!("keep a backup of it somewhere safe, then delete it from this computer, and");
    println!("commit release-key.pub.");
    Ok(())
}
