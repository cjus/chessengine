//! Who a node is: a self-signed certificate and its key, and the fingerprint that names it.
//!
//! There is no certificate authority and nothing to install. Each side makes a certificate the
//! first time it needs one, and the other side **pins its fingerprint** (the SHA-256 of the
//! certificate): that, not any name or signature chain, is what "this is the same node as last
//! time" means. A person confirms a fingerprint once, when pairing (`docs/network-play.md`).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::str::FromStr;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

use crate::files;

/// The SHA-256 of a certificate's DER encoding: how one node recognises another.
///
/// Written as 32 pairs of hexadecimal digits separated by colons, the way browsers and `openssl`
/// show one, and read back with or without the colons or spaces and in either case.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of a certificate.
    pub fn of(certificate: &CertificateDer<'_>) -> Fingerprint {
        Fingerprint(Sha256::digest(certificate.as_ref()).into())
    }

    /// The fingerprint's 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The fingerprint in four lines of eight pairs each, for a person to compare by eye.
    pub fn in_lines(&self) -> String {
        self.0
            .chunks(8)
            .map(|line| {
                line.iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

/// Why text isn't a fingerprint.
#[derive(Debug, PartialEq, Eq)]
pub struct BadFingerprint;

impl fmt::Display for BadFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a fingerprint is 64 hexadecimal digits, with colons or spaces between pairs")
    }
}

impl std::error::Error for BadFingerprint {}

impl FromStr for Fingerprint {
    type Err = BadFingerprint;

    fn from_str(text: &str) -> Result<Fingerprint, BadFingerprint> {
        let digits: Vec<u8> = text
            .chars()
            .filter(|c| !matches!(c, ':' | ' ' | '\n' | '\r' | '\t'))
            .map(|c| c.to_digit(16).map(|d| d as u8).ok_or(BadFingerprint))
            .collect::<Result<_, _>>()?;
        if digits.len() != 64 {
            return Err(BadFingerprint);
        }
        let mut bytes = [0u8; 32];
        for (byte, pair) in bytes.iter_mut().zip(digits.chunks(2)) {
            *byte = pair[0] << 4 | pair[1];
        }
        Ok(Fingerprint(bytes))
    }
}

/// A node's certificate and private key.
pub struct Identity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    fingerprint: Fingerprint,
}

impl fmt::Debug for Identity {
    // The key stays out of any log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Makes a new certificate and key. The certificate names nothing (a fixed placeholder),
    /// since it is recognised by its fingerprint and by nothing in it.
    pub fn generate() -> io::Result<Identity> {
        Ok(Identity::make()?.0)
    }

    /// A new identity, and the PEM text that keeps it.
    fn make() -> io::Result<(Identity, String)> {
        let made = rcgen::generate_simple_self_signed(vec!["cb2026".to_string()])
            .map_err(|e| io::Error::other(format!("making a certificate: {e}")))?;
        let pem = format!("{}{}", made.cert.pem(), made.signing_key.serialize_pem());
        let identity = Identity::from_parts(
            made.cert.der().clone(),
            PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()),
        );
        Ok((identity, pem))
    }

    fn from_parts(certificate: CertificateDer<'static>, key: PrivatePkcs8KeyDer<'static>) -> Self {
        Identity {
            fingerprint: Fingerprint::of(&certificate),
            certificate,
            key,
        }
    }

    /// Reads the identity kept in `path` (a PEM file with the certificate and the key), or makes
    /// one and keeps it there if there is no such file. The file, and the directory made for it,
    /// can be read by their owner only (on systems that have such permissions).
    pub fn load_or_create(path: &Path) -> io::Result<Identity> {
        match fs::read(path) {
            Ok(pem) => Identity::from_pem(&pem).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("{} isn't a usable identity: {e}", path.display()),
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let (identity, pem) = Identity::make()?;
                files::write_private(path, pem.as_bytes())?;
                Ok(identity)
            }
            Err(e) => Err(e),
        }
    }

    fn from_pem(pem: &[u8]) -> io::Result<Identity> {
        let bad = |e: rustls::pki_types::pem::Error| io::Error::new(io::ErrorKind::InvalidData, e);
        Ok(Identity::from_parts(
            CertificateDer::from_pem_slice(pem).map_err(bad)?,
            PrivatePkcs8KeyDer::from_pem_slice(pem).map_err(bad)?,
        ))
    }

    /// The certificate's fingerprint.
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    pub(crate) fn certificate(&self) -> CertificateDer<'static> {
        self.certificate.clone()
    }

    pub(crate) fn key(&self) -> PrivatePkcs8KeyDer<'static> {
        self.key.clone_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_reads_back_in_any_spelling() {
        let identity = Identity::generate().unwrap();
        let fingerprint = identity.fingerprint();
        let shown = fingerprint.to_string();
        assert_eq!(shown.len(), 95);
        assert_eq!(shown.parse(), Ok(fingerprint));
        assert_eq!(shown.to_lowercase().parse(), Ok(fingerprint));
        assert_eq!(shown.replace(':', "").parse(), Ok(fingerprint));
        assert_eq!(fingerprint.in_lines().parse(), Ok(fingerprint));
    }

    #[test]
    fn text_that_isnt_a_fingerprint_is_refused() {
        for text in [
            "",
            "AB:CD",
            &"A".repeat(63),
            &"G".repeat(64),
            &"A".repeat(65),
        ] {
            assert_eq!(text.parse::<Fingerprint>(), Err(BadFingerprint), "{text:?}");
        }
    }

    #[test]
    fn two_identities_differ_and_one_survives_a_restart() {
        let dir = files::temp_dir("identity");
        let path = dir.join("identity.pem");
        let first = Identity::load_or_create(&path).unwrap();
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(first.fingerprint(), again.fingerprint());
        assert_ne!(
            first.fingerprint(),
            Identity::generate().unwrap().fingerprint()
        );
        assert!(!format!("{first:?}").contains("PRIVATE"));
    }

    #[test]
    fn a_damaged_identity_file_is_reported_not_replaced() {
        let dir = files::temp_dir("identity-damaged");
        let path = dir.join("identity.pem");
        fs::write(&path, "not a certificate").unwrap();
        let error = Identity::load_or_create(&path).unwrap_err();
        assert!(error.to_string().contains("identity.pem"), "{error}");
        // The old file is untouched: replacing it would change who this node is.
        assert_eq!(fs::read_to_string(&path).unwrap(), "not a certificate");
    }

    #[cfg(unix)]
    #[test]
    fn the_identity_file_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = files::temp_dir("identity-mode");
        let path = dir.join("keys").join("identity.pem");
        Identity::load_or_create(&path).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
}
