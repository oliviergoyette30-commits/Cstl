//! Federation entre serveurs CSTL (2026-11-25).
//!
//! Le serveur A (`CSTL_SERVER_NAME=acme`) atteste, signe par SA cle de
//! serveur, qu'un de ses agents a telle cle publique :
//!   cstl-federation-v1\nissuer=acme\nagent=bot\npublic_key=<hex>\nissued_at=<s>\nexpires_at=<s>
//! (`purpose=identity_assertion`, demande signee par l'agent lui-meme).
//!
//! Le serveur B, qui fait confiance a A (`CSTL_FEDERATION_PEERS=acme:<cle
//! publique hex de A>,...`), enregistre alors l'agent sous le nom
//! `acme.bot`, avec EXACTEMENT cette cle, sur presentation de l'attestation
//! dans son `agent_register`. Les regles de B s'ecrivent pour `acme.bot`.
//!
//! - L'attestation expire apres `CSTL_FEDERATION_TTL_SECS` (3600) ; au-dela,
//!   B refuse les messages de `acme.bot` (`federation_expired`) jusqu'a une
//!   nouvelle attestation. Une cle revoquee chez A ne peut plus en obtenir :
//!   la revocation atteint B en au plus ce delai.
//! - Chez B, un agent LOCAL ne peut pas prendre un nom `acme.*` : ces noms
//!   sont reserves aux agents du pair.

pub fn message(issuer: &str, agent: &str, public_key: &str, issued_at: i64, expires_at: i64) -> String {
    format!("cstl-federation-v1\nissuer={issuer}\nagent={agent}\npublic_key={public_key}\nissued_at={issued_at}\nexpires_at={expires_at}")
}

pub fn server_name() -> String {
    std::env::var("CSTL_SERVER_NAME").ok().filter(|v| valid_label(v)).unwrap_or_else(|| "cstl".to_string())
}

pub fn ttl_secs() -> i64 {
    std::env::var("CSTL_FEDERATION_TTL_SECS").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(3600)
}

/// Nom de serveur : lettres, chiffres, `_` et `-` (pas de point : il separe
/// le serveur et l'agent dans `acme.bot`).
pub fn valid_label(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Pairs de confiance : `nom:cle_hex,...`. Une entree illisible est ignoree.
pub fn peers() -> Vec<(String, String)> {
    std::env::var("CSTL_FEDERATION_PEERS").unwrap_or_default().split(',')
        .filter_map(|e| {
            let (n, k) = e.trim().split_once(':')?;
            (valid_label(n) && k.len() == 64 && k.chars().all(|c| c.is_ascii_hexdigit())).then(|| (n.to_string(), k.to_ascii_lowercase()))
        })
        .collect()
}

/// Le nom `name` est-il reserve a un pair (`<pair>.<agent>`) ?
pub fn reserved_by_peer(name: &str, peers: &[(String, String)]) -> Option<String> {
    let (prefix, _) = name.split_once('.')?;
    peers.iter().find(|(p, _)| p == prefix).map(|(p, _)| p.clone())
}

/// Attestation presentee a l'enregistrement.
pub struct Assertion<'a> {
    pub issuer: &'a str,
    pub agent: &'a str,
    pub public_key: &'a str,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: &'a str,
}

/// Verifie l'attestation pour un enregistrement sous `name` avec la cle
/// `registering_key`. Ok(expiration) ou Err(raison).
pub fn verify(a: &Assertion, name: &str, registering_key: &str, peers: &[(String, String)], now: i64) -> Result<i64, &'static str> {
    let Some((_, peer_key)) = peers.iter().find(|(p, _)| p == a.issuer) else { return Err("federation_unknown_peer") };
    let msg = message(a.issuer, a.agent, a.public_key, a.issued_at, a.expires_at);
    let ok = (|| {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let pk: [u8; 32] = hex::decode(peer_key).ok()?.try_into().ok()?;
        let sig: [u8; 64] = hex::decode(a.signature).ok()?.try_into().ok()?;
        VerifyingKey::from_bytes(&pk).ok()?.verify(msg.as_bytes(), &Signature::from_bytes(&sig)).ok()
    })();
    if ok.is_none() {
        return Err("federation_signature_invalid");
    }
    if name != format!("{}.{}", a.issuer, a.agent) {
        return Err("federation_name_mismatch");
    }
    if !a.public_key.eq_ignore_ascii_case(registering_key) {
        return Err("federation_key_mismatch");
    }
    if now > a.expires_at || a.issued_at > now + 60 || a.expires_at <= a.issued_at {
        return Err("federation_expired");
    }
    Ok(a.expires_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn a_peer_assertion_registers_exactly_that_agent_with_that_key() {
        let acme = SigningKey::from_bytes(&[7u8; 32]);
        let peers = vec![("acme".to_string(), hex::encode(acme.verifying_key().to_bytes()))];
        let key = "ab".repeat(32);
        let sig = hex::encode(acme.sign(message("acme", "bot", &key, 100, 200).as_bytes()).to_bytes());
        let a = Assertion { issuer: "acme", agent: "bot", public_key: &key, issued_at: 100, expires_at: 200, signature: &sig };
        assert_eq!(verify(&a, "acme.bot", &key, &peers, 150), Ok(200));
        assert_eq!(verify(&a, "acme.admin", &key, &peers, 150), Err("federation_name_mismatch"));
        assert_eq!(verify(&a, "acme.bot", &"cd".repeat(32), &peers, 150), Err("federation_key_mismatch"));
        assert_eq!(verify(&a, "acme.bot", &key, &peers, 201), Err("federation_expired"));
        let forged = Assertion { agent: "admin", ..a };
        assert_eq!(verify(&forged, "acme.admin", &key, &peers, 150), Err("federation_signature_invalid"));
        let evil = Assertion { issuer: "evil", agent: "bot", public_key: &key, issued_at: 100, expires_at: 200, signature: &sig };
        assert_eq!(verify(&evil, "evil.bot", &key, &peers, 150), Err("federation_unknown_peer"));
        assert_eq!(reserved_by_peer("acme.anything", &peers).as_deref(), Some("acme"));
        assert_eq!(reserved_by_peer("local_bot", &peers), None);
        assert!(valid_label("acme-eu") && !valid_label("a.b") && !valid_label(""));
    }
}
