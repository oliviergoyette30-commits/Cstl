//! Identite d'entreprise OIDC a l'enregistrement (2026-11-25).
//!
//! Avec `CSTL_OIDC_ISSUER`, un `agent_register` doit porter
//! `oidc_token=<jeton d'identite JWT>` emis par ce fournisseur, signe (RS256
//! ou ES256) par une cle de son JWKS, pour ce public, non expire, et dont la
//! revendication `CSTL_OIDC_NAME_CLAIM` (`sub` par defaut) est le nom de
//! l'agent. La cle Ed25519 de l'agent est ainsi liee a une identite que
//! l'entreprise gere (et retire) dans son fournisseur.
//!   CSTL_OIDC_ISSUER     emetteur attendu (`iss`) ; active la verification ;
//!   CSTL_OIDC_AUDIENCE   public attendu (`aud`), recommande ;
//!   CSTL_OIDC_JWKS_PATH  fichier JWKS, ou CSTL_OIDC_JWKS_URL (lu a chaque
//!                        enregistrement) ;
//!   CSTL_OIDC_NAME_CLAIM revendication qui porte le nom (defaut `sub`).

use base64::Engine;
use serde_json::Value;

const SKEW: i64 = 60;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub audience: Option<String>,
    pub jwks_path: Option<String>,
    pub jwks_url: Option<String>,
    pub name_claim: String,
}

impl OidcConfig {
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var("CSTL_OIDC_ISSUER").ok().filter(|v| !v.is_empty())?;
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Some(OidcConfig {
            issuer,
            audience: get("CSTL_OIDC_AUDIENCE"),
            jwks_path: get("CSTL_OIDC_JWKS_PATH"),
            jwks_url: get("CSTL_OIDC_JWKS_URL"),
            name_claim: get("CSTL_OIDC_NAME_CLAIM").unwrap_or_else(|| "sub".to_string()),
        })
    }

    /// JWKS du fournisseur (fichier, sinon URL).
    pub async fn jwks(&self) -> Result<Value, &'static str> {
        if let Some(p) = &self.jwks_path {
            let text = std::fs::read_to_string(p).map_err(|_| "oidc_jwks_unreadable")?;
            return serde_json::from_str(&text).map_err(|_| "oidc_jwks_unreadable");
        }
        let url = self.jwks_url.as_ref().ok_or("oidc_jwks_not_configured")?;
        let resp = reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(10)).send().await
            .map_err(|_| "oidc_jwks_unreachable")?;
        resp.json::<Value>().await.map_err(|_| "oidc_jwks_unreadable")
    }
}

fn b64(s: &str) -> Result<Vec<u8>, &'static str> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).map_err(|_| "oidc_token_malformed")
}

/// Verifie `token` pour l'agent `name` ; Err : la raison du refus.
pub fn verify(token: &str, name: &str, cfg: &OidcConfig, jwks: &Value, now: i64) -> Result<(), &'static str> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err("oidc_token_malformed");
    }
    let header: Value = serde_json::from_slice(&b64(parts[0])?).map_err(|_| "oidc_token_malformed")?;
    let claims: Value = serde_json::from_slice(&b64(parts[1])?).map_err(|_| "oidc_token_malformed")?;
    let sig = b64(parts[2])?;
    let signed = format!("{}.{}", parts[0], parts[1]);
    let alg = header["alg"].as_str().unwrap_or("");
    let keys = jwks["keys"].as_array().ok_or("oidc_jwks_unreadable")?;
    let kid = header["kid"].as_str();
    let candidates: Vec<&Value> = keys.iter()
        .filter(|k| kid.is_none() || k["kid"].as_str() == kid)
        .filter(|k| k["use"].as_str().is_none_or(|u| u == "sig"))
        .collect();
    let key = match (kid, candidates.as_slice()) {
        (_, [one]) => *one,
        (None, _) => return Err("oidc_key_ambiguous"),
        (Some(_), []) => return Err("oidc_key_unknown"),
        (Some(_), _) => return Err("oidc_key_ambiguous"),
    };
    let ok = match (alg, key["kty"].as_str()) {
        ("RS256", Some("RSA")) => {
            let n = b64(key["n"].as_str().ok_or("oidc_key_unknown")?)?;
            let e = b64(key["e"].as_str().ok_or("oidc_key_unknown")?)?;
            ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
                .verify(&ring::signature::RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &sig)
                .is_ok()
        }
        ("ES256", Some("EC")) if key["crv"].as_str() == Some("P-256") => {
            let x = b64(key["x"].as_str().ok_or("oidc_key_unknown")?)?;
            let y = b64(key["y"].as_str().ok_or("oidc_key_unknown")?)?;
            if x.len() != 32 || y.len() != 32 {
                return Err("oidc_key_unknown");
            }
            let point: Vec<u8> = [&[4u8][..], &x, &y].concat();
            ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point)
                .verify(signed.as_bytes(), &sig)
                .is_ok()
        }
        // `none`, HS256 (cle partagee), algorithme et cle qui ne vont pas ensemble.
        _ => return Err("oidc_algorithm_refused"),
    };
    if !ok {
        return Err("oidc_signature_invalid");
    }
    if claims["iss"].as_str() != Some(cfg.issuer.as_str()) {
        return Err("oidc_wrong_issuer");
    }
    if let Some(aud) = &cfg.audience {
        let has = match &claims["aud"] {
            Value::String(s) => s == aud,
            Value::Array(a) => a.iter().any(|v| v.as_str() == Some(aud.as_str())),
            _ => false,
        };
        if !has {
            return Err("oidc_wrong_audience");
        }
    }
    let exp = claims["exp"].as_i64().ok_or("oidc_token_malformed")?;
    if now > exp + SKEW {
        return Err("oidc_token_expired");
    }
    if claims["nbf"].as_i64().is_some_and(|nbf| nbf > now + SKEW) {
        return Err("oidc_token_not_yet_valid");
    }
    if claims[cfg.name_claim.as_str()].as_str() != Some(name) {
        return Err("oidc_name_mismatch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};

    fn enc(v: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v)
    }

    fn setup() -> (EcdsaKeyPair, Value, OidcConfig) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let kp = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
        let pk = kp.public_key().as_ref();
        let jwks = serde_json::json!({"keys": [{"kty": "EC", "crv": "P-256", "kid": "k1", "use": "sig",
            "x": enc(&pk[1..33]), "y": enc(&pk[33..65])}]});
        let cfg = OidcConfig { issuer: "https://idp.acme.org".into(), audience: Some("cstl".into()), jwks_path: None,
                               jwks_url: None, name_claim: "sub".into() };
        (kp, jwks, cfg)
    }

    fn token(kp: &EcdsaKeyPair, header: Value, claims: Value) -> String {
        let signed = format!("{}.{}", enc(header.to_string().as_bytes()), enc(claims.to_string().as_bytes()));
        let sig = kp.sign(&ring::rand::SystemRandom::new(), signed.as_bytes()).unwrap();
        format!("{signed}.{}", enc(sig.as_ref()))
    }

    #[test]
    fn a_valid_id_token_binds_the_agent_name_and_everything_else_is_refused() {
        let (kp, jwks, cfg) = setup();
        let h = serde_json::json!({"alg": "ES256", "kid": "k1"});
        let good = serde_json::json!({"iss": "https://idp.acme.org", "aud": ["cstl", "x"], "sub": "agent_a", "exp": 2000});
        assert_eq!(verify(&token(&kp, h.clone(), good.clone()), "agent_a", &cfg, &jwks, 1000), Ok(()));
        assert_eq!(verify(&token(&kp, h.clone(), good.clone()), "agent_b", &cfg, &jwks, 1000), Err("oidc_name_mismatch"));
        assert_eq!(verify(&token(&kp, h.clone(), good.clone()), "agent_a", &cfg, &jwks, 3000), Err("oidc_token_expired"));
        let mut other = good.clone();
        other["iss"] = "https://evil".into();
        assert_eq!(verify(&token(&kp, h.clone(), other), "agent_a", &cfg, &jwks, 1000), Err("oidc_wrong_issuer"));
        let mut other = good.clone();
        other["aud"] = "autre".into();
        assert_eq!(verify(&token(&kp, h.clone(), other), "agent_a", &cfg, &jwks, 1000), Err("oidc_wrong_audience"));
        // Revendications modifiees apres signature.
        let t = token(&kp, h.clone(), good.clone());
        let mut p: Vec<&str> = t.split('.').collect();
        let forged = enc(serde_json::json!({"iss": "https://idp.acme.org", "aud": "cstl", "sub": "admin", "exp": 2000}).to_string().as_bytes());
        p[1] = &forged;
        assert_eq!(verify(&p.join("."), "admin", &cfg, &jwks, 1000), Err("oidc_signature_invalid"));
        // alg=none, HS256, cle inconnue, jeton illisible.
        assert_eq!(verify(&token(&kp, serde_json::json!({"alg": "none", "kid": "k1"}), good.clone()), "agent_a", &cfg, &jwks, 1000), Err("oidc_algorithm_refused"));
        assert_eq!(verify(&token(&kp, serde_json::json!({"alg": "HS256", "kid": "k1"}), good.clone()), "agent_a", &cfg, &jwks, 1000), Err("oidc_algorithm_refused"));
        assert_eq!(verify(&token(&kp, serde_json::json!({"alg": "ES256", "kid": "k9"}), good.clone()), "agent_a", &cfg, &jwks, 1000), Err("oidc_key_unknown"));
        assert_eq!(verify("abc", "agent_a", &cfg, &jwks, 1000), Err("oidc_token_malformed"));
    }
}
