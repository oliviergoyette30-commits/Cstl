//! Identite de charge de travail SPIFFE (2026-11-25).
//!
//! En TLS mutuel, le certificat client porte un identifiant SPIFFE dans son
//! SAN URI : `spiffe://<domaine de confiance>/<chemin>/<nom>`. Avec
//! `CSTL_SPIFFE_TRUST_DOMAIN=<domaine>`, le DERNIER segment du chemin est le
//! nom d'agent de la connexion : chaque message doit avoir `sender=<ce nom>`,
//! enregistrement compris. Une cle Ed25519 volee ne sert alors a rien depuis
//! une autre charge de travail. `CSTL_SPIFFE_REQUIRED=true` refuse aussi les
//! connexions sans identifiant SPIFFE.
//!
//! Exception : `token_status` (lecture seule, non signe, pour le systeme qui
//! execute) n'a pas de nom d'agent a verifier.

/// Identite tiree du certificat client.
#[derive(Debug, Clone, PartialEq)]
pub enum PeerIdentity {
    /// Pas de TLS mutuel, ou certificat sans identifiant SPIFFE.
    None,
    /// Nom d'agent tire d'un identifiant de notre domaine de confiance.
    Agent(String),
    /// Identifiant SPIFFE d'un autre domaine (ou illisible) : jamais accepte.
    Foreign(String),
}

/// Identifiants SPIFFE (SAN URI `spiffe://...`) d'un certificat DER.
pub fn spiffe_ids(der: &[u8]) -> Vec<String> {
    use x509_parser::extensions::GeneralName;
    let Ok((_, cert)) = x509_parser::parse_x509_certificate(der) else { return Vec::new() };
    let Ok(Some(san)) = cert.subject_alternative_name() else { return Vec::new() };
    san.value
        .general_names
        .iter()
        .filter_map(|g| match g {
            GeneralName::URI(u) if u.starts_with("spiffe://") => Some(u.to_string()),
            _ => None,
        })
        .collect()
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Nom d'agent d'un identifiant SPIFFE de `trust_domain`, ou None.
pub fn agent_name(spiffe_id: &str, trust_domain: &str) -> Option<String> {
    let rest = spiffe_id.strip_prefix("spiffe://")?;
    let (domain, path) = rest.split_once('/')?;
    if !domain.eq_ignore_ascii_case(trust_domain) || path.contains("//") || path.contains('?') || path.contains('#') {
        return None;
    }
    let last = path.rsplit('/').next()?;
    valid_name(last).then(|| last.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct SpiffePolicy {
    pub trust_domain: Option<String>,
    pub required: bool,
}

impl SpiffePolicy {
    pub fn from_env() -> Self {
        SpiffePolicy {
            trust_domain: std::env::var("CSTL_SPIFFE_TRUST_DOMAIN").ok().filter(|v| !v.is_empty()),
            required: std::env::var("CSTL_SPIFFE_REQUIRED").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false),
        }
    }

    /// Identite de la connexion, d'apres les identifiants du certificat client.
    pub fn identify(&self, ids: &[String]) -> PeerIdentity {
        let Some(td) = &self.trust_domain else { return PeerIdentity::None };
        match ids.first() {
            None => PeerIdentity::None,
            Some(id) => match agent_name(id, td) {
                Some(name) if ids.len() == 1 => PeerIdentity::Agent(name),
                _ => PeerIdentity::Foreign(id.clone()),
            },
        }
    }

    /// Le message de `sender` est-il permis sur cette connexion ?
    pub fn check(&self, peer: &PeerIdentity, sender: &str) -> Result<(), &'static str> {
        if self.trust_domain.is_none() {
            return Ok(());
        }
        match peer {
            PeerIdentity::Agent(name) if name == sender => Ok(()),
            PeerIdentity::Agent(_) => Err("spiffe_identity_mismatch"),
            PeerIdentity::Foreign(_) => Err("spiffe_foreign_identity"),
            PeerIdentity::None if self.required => Err("spiffe_identity_required"),
            PeerIdentity::None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spiffe_id_maps_to_the_agent_name_of_our_trust_domain_only() {
        assert_eq!(agent_name("spiffe://acme.org/ns/prod/sa/agent_support", "acme.org").as_deref(), Some("agent_support"));
        assert_eq!(agent_name("spiffe://ACME.org/agent_x", "acme.org").as_deref(), Some("agent_x"));
        assert_eq!(agent_name("spiffe://evil.org/agent_support", "acme.org"), None);
        assert_eq!(agent_name("spiffe://acme.org/a b", "acme.org"), None);
        assert_eq!(agent_name("spiffe://acme.org", "acme.org"), None);
        assert_eq!(agent_name("https://acme.org/agent", "acme.org"), None);
    }

    #[test]
    fn the_connection_identity_binds_the_sender() {
        let p = SpiffePolicy { trust_domain: Some("acme.org".into()), required: false };
        let me = p.identify(&["spiffe://acme.org/sa/bot".into()]);
        assert_eq!(me, PeerIdentity::Agent("bot".into()));
        assert_eq!(p.check(&me, "bot"), Ok(()));
        assert_eq!(p.check(&me, "owner"), Err("spiffe_identity_mismatch"));
        let foreign = p.identify(&["spiffe://evil.org/sa/bot".into()]);
        assert_eq!(p.check(&foreign, "bot"), Err("spiffe_foreign_identity"));
        let two = p.identify(&["spiffe://acme.org/a".into(), "spiffe://acme.org/b".into()]);
        assert!(matches!(two, PeerIdentity::Foreign(_)), "deux identifiants : ambigu, refuse");
        assert_eq!(p.check(&PeerIdentity::None, "bot"), Ok(()));
        let strict = SpiffePolicy { required: true, ..p.clone() };
        assert_eq!(strict.check(&PeerIdentity::None, "bot"), Err("spiffe_identity_required"));
        let off = SpiffePolicy::default();
        assert_eq!(off.check(&PeerIdentity::Foreign("x".into()), "bot"), Ok(()));
    }

    #[test]
    fn reads_the_uri_san_of_a_real_certificate() {
        use rcgen::{CertificateParams, KeyPair, SanType};
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.subject_alt_names = vec![
            SanType::URI("spiffe://acme.org/sa/bot".try_into().unwrap()),
            SanType::DnsName("bot.local".try_into().unwrap()),
        ];
        let cert = params.self_signed(&key).unwrap();
        assert_eq!(spiffe_ids(cert.der()), vec!["spiffe://acme.org/sa/bot".to_string()]);
        assert!(spiffe_ids(b"not a certificate").is_empty());
    }
}
