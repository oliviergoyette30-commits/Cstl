//! tests/conformance_vectors_test.rs -- le serveur Rust verifie les memes
//! vecteurs que Python (conformance/vectors.json, 2026-11-25).
//!
//! Signature : le message JSON du vecteur passe par l'entree JSON
//! (`json_gateway::json_to_wire`), puis par le vrai lecteur ; les octets
//! canoniques doivent etre exactement ceux du vecteur, et la signature doit
//! etre acceptee par `signing::check_signature`.
//! Jetons : le message canonique et chaque verdict doivent etre ceux du
//! vecteur.

use cstl_parser::server::audit::signing_bytes;
use cstl_parser::server::json_gateway::json_to_wire;
use cstl_parser::server::parser::parse_payload;
use cstl_parser::server::server_key::{verify_authorization, Authorization, AuthzError};
use cstl_parser::signing::{check_signature, SignatureCheck};

fn vectors() -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/conformance/vectors.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("conformance/vectors.json")).unwrap()
}

#[test]
fn signing_vectors_match_the_server() {
    let v = vectors();
    for case in v["signing"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let payload = parse_payload(&json_to_wire(&case["message"]).unwrap()).unwrap();
        let canon = String::from_utf8(signing_bytes(&payload)).unwrap();
        assert_eq!(canon, case["canonical"].as_str().unwrap(), "octets canoniques, vecteur {name}");
        assert_eq!(check_signature(&payload), SignatureCheck::Valid, "signature, vecteur {name}");
    }
}

#[test]
fn token_vectors_match_the_server() {
    let v = vectors();
    let t = &v["token"];
    let f = &t["fields"];
    let s = |k: &str| f[k].as_str().unwrap().to_string();
    let base = Authorization {
        agent: s("agent"),
        action: s("action"),
        request_id: s("request_id"),
        issued_at: f["issued_at"].as_u64().unwrap(),
        expires_at: f["expires_at"].as_u64().unwrap(),
        audit_hash: s("audit_hash"),
        amount: f["amount"].as_str().map(String::from),
    };
    assert_eq!(base.message(), t["message"].as_str().unwrap());
    let key = t["server_public_key"].as_str().unwrap();
    let sig = t["signature"].as_str().unwrap();
    for c in t["verdicts"].as_array().unwrap() {
        let mut a = base.clone();
        if let Some(amount) = c["token"]["amount"].as_str() {
            a.amount = Some(amount.to_string());
        }
        let got = verify_authorization(
            key, &a, sig,
            c["action"].as_str().unwrap(),
            c["agent"].as_str(),
            c["amount"].as_str(),
            c["now"].as_u64().unwrap(),
            t["skew"].as_u64().unwrap(),
        );
        let got = match got {
            Ok(()) => "ok",
            Err(AuthzError::BadSignature) => "bad_signature",
            Err(AuthzError::BadKey) => "bad_key",
            Err(AuthzError::WrongAction) => "wrong_action",
            Err(AuthzError::WrongAgent) => "wrong_agent",
            Err(AuthzError::WrongAmount) => "wrong_amount",
            Err(AuthzError::Expired) => "expired",
            Err(AuthzError::NotYetValid) => "not_yet_valid",
        };
        assert_eq!(got, c["expect"].as_str().unwrap(), "verdict : {}", c["case"]);
    }
}
