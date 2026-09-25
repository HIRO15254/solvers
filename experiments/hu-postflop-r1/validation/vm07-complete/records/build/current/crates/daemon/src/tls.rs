//! TLS configuration and the rule about exposing the daemon.
//!
//! A bearer token proves who is asking; it does nothing about who is
//! listening. On a plain HTTP connection the token itself crosses the wire
//! in a header, so anyone on the path can take it and then submit runs,
//! cancel them, and download solutions. That is why a non-loopback bind
//! without TLS is refused rather than warned about (R7).

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use anyhow::{Context, Result};
use tiny_http::SslConfig;

/// The certificate and key to serve with.
pub struct Tls {
    pub certificate: Vec<u8>,
    pub private_key: Vec<u8>,
}

impl Tls {
    pub fn load(certificate: &Path, private_key: &Path) -> Result<Self> {
        Ok(Self {
            certificate: std::fs::read(certificate).with_context(|| {
                format!("reading the TLS certificate {}", certificate.display())
            })?,
            private_key: std::fs::read(private_key).with_context(|| {
                format!("reading the TLS private key {}", private_key.display())
            })?,
        })
    }

    pub fn into_config(self) -> SslConfig {
        SslConfig {
            certificate: self.certificate,
            private_key: self.private_key,
        }
    }
}

/// Whether every address `bind` resolves to is loopback.
///
/// A hostname can resolve to several addresses, and reaching the daemon over
/// any one of them is enough, so this is only true when they all are.
pub fn is_loopback(bind: &str) -> Result<bool> {
    let addresses: Vec<SocketAddr> = bind
        .to_socket_addrs()
        .with_context(|| format!("resolving the bind address {bind}"))?
        .collect();
    if addresses.is_empty() {
        anyhow::bail!("the bind address {bind} resolved to nothing");
    }
    Ok(addresses.iter().all(|address| address.ip().is_loopback()))
}

/// Refuses a configuration that would put the token on the wire in clear.
pub fn check_exposure(bind: &str, tls: bool) -> Result<()> {
    if tls || is_loopback(bind)? {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to bind {bind} without TLS: the bearer token would cross the network \
         in clear. Pass --tls-cert and --tls-key, or bind a loopback address and reach \
         it through an SSH tunnel."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_addresses_are_recognized() {
        assert!(is_loopback("127.0.0.1:38127").unwrap());
        assert!(is_loopback("localhost:38127").unwrap());
        assert!(is_loopback("[::1]:38127").unwrap());
        assert!(!is_loopback("0.0.0.0:38127").unwrap());
    }

    /// The whole point: a token on a clear network connection is a token
    /// anyone on the path can take and use.
    #[test]
    fn a_public_bind_without_tls_is_refused() {
        let error = check_exposure("0.0.0.0:38127", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without TLS"), "{error}");
        assert!(error.contains("--tls-cert"), "{error}");

        check_exposure("0.0.0.0:38127", true).expect("TLS makes a public bind acceptable");
        check_exposure("127.0.0.1:38127", false).expect("loopback needs no TLS");
    }

    #[test]
    fn an_unresolvable_bind_is_an_error() {
        assert!(check_exposure("no-such-host.invalid:38127", false).is_err());
    }
}
