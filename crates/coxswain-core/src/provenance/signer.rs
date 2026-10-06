//! Who signed: the identity and claims in a Sigstore (Fulcio) certificate. Parsed only, never
//! verified: the chain, the signature and the log are not checked.

use serde::Serialize;
use x509_parser::prelude::*;

/// Fulcio's claims, from the extensions under 1.3.6.1.4.1.57264.1. Docs:
/// <https://github.com/sigstore/fulcio/blob/main/docs/oid-info.md>.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Claim {
    Issuer,
    BuildSignerUri,
    BuildSignerDigest,
    RunnerEnvironment,
    SourceRepositoryUri,
    SourceRepositoryDigest,
    SourceRepositoryRef,
    SourceRepositoryIdentifier,
    SourceRepositoryOwnerUri,
    SourceRepositoryOwnerIdentifier,
    BuildConfigUri,
    BuildConfigDigest,
    BuildTrigger,
    RunInvocationUri,
    SourceRepositoryVisibility,
    /// Legacy (.4): the workflow's name. Nothing newer replaces it.
    WorkflowName,
    /// Legacy (.5): `owner/repo`, where newer certificates have SourceRepositoryUri.
    Repository,
}

const PREFIX: &str = "1.3.6.1.4.1.57264.1.";

/// The claim of an extension's last arc. The legacy ones (1–6) are raw text; 8 and on are DER.
fn claim(arc: u32) -> Option<(Claim, bool)> {
    use Claim::*;
    Some(match arc {
        1 => (Issuer, true),
        2 => (BuildTrigger, true),
        3 => (SourceRepositoryDigest, true),
        4 => (WorkflowName, true),
        5 => (Repository, true),
        6 => (SourceRepositoryRef, true),
        8 => (Issuer, false),
        9 => (BuildSignerUri, false),
        10 => (BuildSignerDigest, false),
        11 => (RunnerEnvironment, false),
        12 => (SourceRepositoryUri, false),
        13 => (SourceRepositoryDigest, false),
        14 => (SourceRepositoryRef, false),
        15 => (SourceRepositoryIdentifier, false),
        16 => (SourceRepositoryOwnerUri, false),
        17 => (SourceRepositoryOwnerIdentifier, false),
        18 => (BuildConfigUri, false),
        19 => (BuildConfigDigest, false),
        20 => (BuildTrigger, false),
        21 => (RunInvocationUri, false),
        22 => (SourceRepositoryVisibility, false),
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signer {
    /// The certificate's subject alternative name: a workflow URI or an e-mail address.
    pub identity: String,
    /// Who issued the certificate, e.g. "O=sigstore.dev, CN=sigstore-intermediate".
    pub ca: String,
    /// Seconds since 1970. A Fulcio certificate lives about ten minutes.
    pub not_before: i64,
    pub not_after: i64,
    /// Sorted by claim; the newer extension wins over the legacy one.
    pub claims: Vec<(Claim, String)>,
}

impl Signer {
    pub fn get(&self, c: Claim) -> Option<&str> {
        self.claims.iter().find(|(k, _)| *k == c).map(|(_, v)| v.as_str())
    }
}

/// Reads a DER certificate.
pub fn read(der: &[u8]) -> Result<Signer, String> {
    let (_, cert) = parse_x509_certificate(der).map_err(|e| e.to_string())?;
    let identity = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .and_then(|san| {
            san.value.general_names.iter().find_map(|n| match n {
                GeneralName::URI(s) | GeneralName::RFC822Name(s) => Some(s.to_string()),
                _ => None,
            })
        })
        .unwrap_or_default();
    let mut claims: Vec<(Claim, String, bool)> = Vec::new();
    for ext in cert.extensions() {
        let oid = ext.oid.to_id_string();
        let Some((c, legacy)) = oid.strip_prefix(PREFIX).and_then(|a| a.parse().ok()).and_then(claim) else { continue };
        let value = if legacy { String::from_utf8_lossy(ext.value).into_owned() } else { utf8_string(ext.value) };
        match claims.iter_mut().find(|(k, ..)| *k == c) {
            Some(slot) if slot.2 && !legacy => *slot = (c, value, legacy),
            Some(_) => {}
            None => claims.push((c, value, legacy)),
        }
    }
    claims.sort_by_key(|(c, ..)| *c);
    Ok(Signer {
        identity,
        ca: cert.issuer().to_string(),
        not_before: cert.validity().not_before.timestamp(),
        not_after: cert.validity().not_after.timestamp(),
        claims: claims.into_iter().map(|(c, v, _)| (c, v)).collect(),
    })
}

/// A DER UTF8String's text; anything else as it is, lossily.
fn utf8_string(v: &[u8]) -> String {
    if let Ok((_, s)) = x509_parser::der_parser::der::parse_der_utf8string(v)
        && let Ok(s) = s.as_str()
    {
        return s.to_string();
    }
    String::from_utf8_lossy(v).into_owned()
}
