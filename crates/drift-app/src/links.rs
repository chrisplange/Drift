//! Drift's addresses on the web: the website (with the user guide) and the public repository
//! for releases and issue reports. The source repository is private, so nothing links there.

/// An address on Drift's website. Keep the host in step with the `SITE_URL` repository variable.
macro_rules! site {
    ($path:literal) => {
        concat!("https://drift-website-c9y.pages.dev", $path)
    };
}

/// The website's front page.
pub(crate) const WEBSITE: &str = site!("/");
/// The user guide.
pub(crate) const GUIDE: &str = site!("/guide/");
/// Where to download Drift by hand (the updater links it; the web build has no updater).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const DOWNLOAD: &str = site!("/download/");
/// Drift's licence, in plain words, with the agreement.
pub(crate) const LICENCE: &str = site!("/licence/");
/// Where to report a problem or ask for a feature.
pub(crate) const REPORT: &str = "https://github.com/chrisplange/drift-releases/issues/new/choose";
