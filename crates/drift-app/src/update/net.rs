//! HTTPS with ureq and rustls, checked against the operating system's certificates (so it
//! works behind the proxies some offices use).

use std::io::Read as _;
use std::sync::Arc;
use std::time::Duration;

use super::Http;

pub(crate) struct Agent(ureq::Agent);

impl Agent {
    pub(crate) fn new() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(format!("Drift/{}", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .new_agent();
        Self(agent)
    }
}

impl Http for Agent {
    fn get(
        &self,
        url: &str,
        limit: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<Vec<u8>, String> {
        let mut response = self
            .0
            .get(url)
            .header(
                "Accept",
                "application/vnd.github+json, application/octet-stream",
            )
            .call()
            .map_err(describe)?;
        let total = response.body().content_length();
        let mut reader = response.body_mut().with_config().limit(limit).reader();
        let mut bytes = Vec::with_capacity(total.unwrap_or(0).min(limit) as usize);
        let mut chunk = vec![0; 64 << 10];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| describe_io(&e))?;
            if n == 0 {
                return Ok(bytes);
            }
            bytes.extend_from_slice(&chunk[..n]);
            if !progress(bytes.len() as u64, total) {
                return Err("cancelled".into());
            }
        }
    }
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(403 | 429) => "GitHub is limiting requests: try again later".into(),
        ureq::Error::StatusCode(code) => format!("GitHub answered {code}"),
        ureq::Error::Timeout(_) => "no answer from GitHub".into(),
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
            "no connection to GitHub".into()
        }
        e => e.to_string(),
    }
}

fn describe_io(e: &std::io::Error) -> String {
    format!("the download stopped: {e}")
}
