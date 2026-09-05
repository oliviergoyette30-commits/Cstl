//! src/signing.rs — signature Ed25519 des messages (Couche 2/sécurité, v1,
//! 2026-09-04).
//!
//! Avant ce module, `sender`/`receiver` dans le wire format CSTL étaient de
//! simples chaînes de texte sans AUCUNE vérification cryptographique —
//! n'importe qui pouvait se prétendre n'importe quel agent sur le TCP en
//! clair. Ça ferme exactement ce que le marché nomme en premier (OWASP Top
//! 10 for Agentic Applications 2026: ASI03 Identity & Privilege Abuse,
//! ASI07 Insecure Inter-Agent Communication), sans réinventer OAuth (trop
//! lourd pour ce format texte déterministe) — une signature Ed25519 par
//! message, vérifiée contre la clé que le message lui-même revendique dans
//! `META.public_key`, suffit à empêcher qu'un tiers forge un message au nom
//! d'une identité déjà connue.
//!
//! Portée v1, décision explicite: **optionnelle globalement, obligatoire
//! seulement pour un expéditeur déjà enregistré dans l'`AgentRegistry` avec
//! une `public_key`**. Sinon les payloads non signés existants (les 148
//! tests, les smoke-tests, alice/bob legacy) casseraient tous pour fermer un
//! risque qui ne concerne, dans les faits, que les identités déjà établies.
//!
//! Champs de wire format:
//! - `META.public_key=<hex, 64 caractères>` — clé publique Ed25519 (32 octets)
//! - `INTENT_PAYLOAD.signature=<hex, 128 caractères>` — signature (64 octets)
//!
//! Ce qui est signé: `super::server::audit::signing_bytes(payload)` — une
//! canonicalisation dupliquée (délibérément, pas un refactor) de
//! `canonical_hash`, qui exclut le champ `signature` lui-même (un message ne
//! peut pas se signer) et `PARENT_HASH`, et inclut `public_key` (lie la
//! signature à la clé revendiquée).

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use crate::server::parser::CstlPayload;
use crate::server::audit::signing_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureCheck {
    /// Ni `META.public_key` ni `INTENT_PAYLOAD.signature` ne sont présents.
    NotPresent,
    Valid,
    Invalid(String),
}

fn decode_hex_fixed<const N: usize>(hex_str: &str) -> Result<[u8; N], &'static str> {
    let bytes = hex::decode(hex_str).map_err(|_| "invalid_hex")?;
    bytes.try_into().map_err(|_| "bad_length")
}

/// Coeur partage de la verification: un message brut, une cle publique hex,
/// une signature hex -- sans savoir d'ou l'appelant a tire la cle (le
/// message lui-meme via META.public_key pour `check_signature`, ou le
/// registre pour `check_rotation_signature` ci-dessous). Extrait le
/// 2026-09-04 (Couche 7, item #1 de la liste des choses a faire) pour que
/// les deux appelants partagent EXACTEMENT la meme logique de decodage/
/// verification plutot que de la dupliquer avec un risque de divergence.
fn verify_raw(message: &[u8], public_key_hex: &str, signature_hex: &str) -> SignatureCheck {
    let public_key_bytes: [u8; 32] = match decode_hex_fixed(public_key_hex) {
        Ok(b) => b,
        Err("invalid_hex") => return SignatureCheck::Invalid("invalid_hex".to_string()),
        Err(_) => return SignatureCheck::Invalid("bad_public_key_length".to_string()),
    };
    let signature_bytes: [u8; 64] = match decode_hex_fixed(signature_hex) {
        Ok(b) => b,
        Err("invalid_hex") => return SignatureCheck::Invalid("invalid_hex".to_string()),
        Err(_) => return SignatureCheck::Invalid("bad_signature_length".to_string()),
    };

    let verifying_key = match VerifyingKey::from_bytes(&public_key_bytes) {
        Ok(k) => k,
        Err(_) => return SignatureCheck::Invalid("bad_public_key_length".to_string()),
    };
    let signature = Signature::from_bytes(&signature_bytes);

    match verifying_key.verify(message, &signature) {
        Ok(()) => SignatureCheck::Valid,
        Err(_) => SignatureCheck::Invalid("verification_failed".to_string()),
    }
}

/// Vérifie la signature d'un payload. Ne regarde JAMAIS le registre
/// d'agents — la correction cryptographique ne dépend que du message
/// lui-même ; c'est à l'appelant (handler.rs) de décider, séparément, si
/// une signature était OBLIGATOIRE pour cet expéditeur.
pub fn check_signature(payload: &CstlPayload) -> SignatureCheck {
    let public_key_hex = payload.meta.get("public_key");
    let signature_hex = payload.intent.get("signature");

    let (public_key_hex, signature_hex) = match (public_key_hex, signature_hex) {
        (Some(pk), Some(sig)) => (pk, sig),
        (None, None) => return SignatureCheck::NotPresent,
        _ => {
            // Un seul des deux champs présent: ni "absent" (on ne peut pas
            // vérifier sans les deux) ni silencieusement ignoré -- signalé
            // comme invalide plutôt que traité comme NotPresent, pour ne
            // pas laisser passer un message qui ESSAIE de se signer mais
            // oublie un champ.
            return SignatureCheck::Invalid("incomplete_signature_fields".to_string());
        }
    };

    verify_raw(&signing_bytes(payload), public_key_hex, signature_hex)
}

/// Preuve de rotation de clé (Couche 7, 2026-09-04, item #1 de la liste des
/// choses à faire). Trouvaille: `purpose=agent_register` acceptait un
/// ré-enregistrement avec une NOUVELLE clé publique sur simple auto-
/// signature -- celle-ci prouve seulement "je possède cette nouvelle clé",
/// jamais "je suis le même agent que celui déjà enregistré sous ce nom".
/// N'importe qui connaissant juste le NOM d'un agent déjà enregistré
/// pouvait donc lui voler son identité en soumettant un nouvel
/// `agent_register` avec sa propre paire de clés.
///
/// Corrigé: quand la clé publique embarquée diffère de celle déjà
/// enregistrée pour ce nom, le message doit AUSSI porter
/// `INTENT_PAYLOAD.rotation_signature=<hex 128 car.>` — une signature du
/// MÊME message (`signing_bytes`, qui exclut `signature` ET
/// `rotation_signature`) mais faite avec l'ANCIENNE clé privée. Ça prouve
/// la possession simultanée de l'ancienne ET de la nouvelle clé -- une
/// vraie rotation, pas juste une revendication. `old_public_key_hex` vient
/// TOUJOURS du registre (jamais du message lui-même, qui pourrait mentir).
pub fn check_rotation_signature(payload: &CstlPayload, old_public_key_hex: &str) -> SignatureCheck {
    let rotation_signature_hex = match payload.intent.get("rotation_signature") {
        Some(sig) => sig,
        None => return SignatureCheck::NotPresent,
    };
    verify_raw(&signing_bytes(payload), old_public_key_hex, rotation_signature_hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::HashMap;

    fn mk_payload(extra_meta: &[(&str, &str)], extra_intent: &[(&str, &str)]) -> CstlPayload {
        let mut meta = HashMap::new();
        meta.insert("encoder".to_string(), "Agent".to_string());
        meta.insert("produced_by".to_string(), "Test".to_string());
        for (k, v) in extra_meta {
            meta.insert(k.to_string(), v.to_string());
        }
        let mut intent = HashMap::new();
        intent.insert("purpose".to_string(), "test".to_string());
        intent.insert("sender".to_string(), "signer_agent".to_string());
        intent.insert("receiver".to_string(), "server".to_string());
        for (k, v) in extra_intent {
            intent.insert(k.to_string(), v.to_string());
        }
        CstlPayload {
            version: "v5.0.0".to_string(),
            mode: "A".to_string(),
            meta,
            intent,
            relations: vec![],
            defines: vec![],
            parse_warnings: vec![],
            raw: String::new(),
        }
    }

    fn signed_payload(signing_key: &SigningKey, pubkey_hex: &str) -> CstlPayload {
        // On construit d'abord le payload SANS signature pour calculer
        // signing_bytes, puis on l'ajoute -- signing_bytes exclut le champ
        // signature de toute facon, mais ca refletela sequence reelle
        // (le client ne connait pas la signature avant de l'avoir calculee).
        let unsigned = mk_payload(&[("public_key", pubkey_hex)], &[]);
        let signature = signing_key.sign(&signing_bytes(&unsigned));
        mk_payload(&[("public_key", pubkey_hex)], &[("signature", &hex::encode(signature.to_bytes()))])
    }

    #[test]
    fn test_no_signature_fields_is_not_present() {
        let payload = mk_payload(&[], &[]);
        assert_eq!(check_signature(&payload), SignatureCheck::NotPresent);
    }

    #[test]
    fn test_only_public_key_no_signature_is_invalid() {
        let payload = mk_payload(&[("public_key", &"aa".repeat(32))], &[]);
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("incomplete_signature_fields".to_string()));
    }

    #[test]
    fn test_only_signature_no_public_key_is_invalid() {
        let payload = mk_payload(&[], &[("signature", &"aa".repeat(64))]);
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("incomplete_signature_fields".to_string()));
    }

    #[test]
    fn test_valid_signature_verifies() {
        let mut csprng = rand_core_shim();
        let signing_key = SigningKey::generate(&mut csprng);
        let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
        let payload = signed_payload(&signing_key, &pubkey_hex);
        assert_eq!(check_signature(&payload), SignatureCheck::Valid);
    }

    #[test]
    fn test_tampered_payload_fails_verification() {
        let mut csprng = rand_core_shim();
        let signing_key = SigningKey::generate(&mut csprng);
        let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
        let mut payload = signed_payload(&signing_key, &pubkey_hex);
        payload.intent.insert("purpose".to_string(), "tampered".to_string());
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("verification_failed".to_string()));
    }

    #[test]
    fn test_signature_from_wrong_key_fails() {
        let mut csprng = rand_core_shim();
        let real_key = SigningKey::generate(&mut csprng);
        let other_key = SigningKey::generate(&mut csprng);
        let claimed_pubkey_hex = hex::encode(real_key.verifying_key().to_bytes());
        // Signe avec other_key mais revendique la cle publique de real_key.
        let unsigned = mk_payload(&[("public_key", &claimed_pubkey_hex)], &[]);
        let bad_signature = other_key.sign(&signing_bytes(&unsigned));
        let payload = mk_payload(
            &[("public_key", &claimed_pubkey_hex)],
            &[("signature", &hex::encode(bad_signature.to_bytes()))],
        );
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("verification_failed".to_string()));
    }

    #[test]
    fn test_invalid_hex_public_key() {
        let payload = mk_payload(&[("public_key", "not_hex_zzz")], &[("signature", &"aa".repeat(64))]);
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("invalid_hex".to_string()));
    }

    #[test]
    fn test_wrong_length_public_key() {
        let payload = mk_payload(&[("public_key", "aabb")], &[("signature", &"aa".repeat(64))]);
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("bad_public_key_length".to_string()));
    }

    #[test]
    fn test_wrong_length_signature() {
        let payload = mk_payload(&[("public_key", &"aa".repeat(32))], &[("signature", "aabb")]);
        assert_eq!(check_signature(&payload), SignatureCheck::Invalid("bad_signature_length".to_string()));
    }

    // ── check_rotation_signature (Couche 7, item #1) ──

    #[test]
    fn test_rotation_signature_absent_is_not_present() {
        let payload = mk_payload(&[("public_key", &"bb".repeat(32))], &[("signature", &"aa".repeat(64))]);
        let old_key = "cc".repeat(32);
        assert_eq!(check_rotation_signature(&payload, &old_key), SignatureCheck::NotPresent);
    }

    #[test]
    fn test_rotation_signature_valid_with_old_key() {
        let mut csprng = rand_core_shim();
        let old_key = SigningKey::generate(&mut csprng);
        let new_key = SigningKey::generate(&mut csprng);
        let old_pubkey_hex = hex::encode(old_key.verifying_key().to_bytes());
        let new_pubkey_hex = hex::encode(new_key.verifying_key().to_bytes());

        // Le message final porte les DEUX signatures: l'auto-signature avec
        // la nouvelle cle (deja teste ailleurs) ET rotation_signature avec
        // l'ancienne -- signing_bytes exclut les deux champs, donc les deux
        // signatures portent sur EXACTEMENT le meme message.
        let unsigned = mk_payload(&[("public_key", &new_pubkey_hex)], &[]);
        let message = signing_bytes(&unsigned);
        let self_sig = new_key.sign(&message);
        let rotation_sig = old_key.sign(&message);
        let payload = mk_payload(
            &[("public_key", &new_pubkey_hex)],
            &[
                ("signature", &hex::encode(self_sig.to_bytes())),
                ("rotation_signature", &hex::encode(rotation_sig.to_bytes())),
            ],
        );

        assert_eq!(check_signature(&payload), SignatureCheck::Valid);
        assert_eq!(check_rotation_signature(&payload, &old_pubkey_hex), SignatureCheck::Valid);
    }

    #[test]
    fn test_rotation_signature_from_wrong_old_key_fails() {
        let mut csprng = rand_core_shim();
        let real_old_key = SigningKey::generate(&mut csprng);
        let impostor_key = SigningKey::generate(&mut csprng);
        let new_key = SigningKey::generate(&mut csprng);
        let new_pubkey_hex = hex::encode(new_key.verifying_key().to_bytes());
        let real_old_pubkey_hex = hex::encode(real_old_key.verifying_key().to_bytes());

        let unsigned = mk_payload(&[("public_key", &new_pubkey_hex)], &[]);
        let message = signing_bytes(&unsigned);
        // Un attaquant qui ne possede PAS la vraie ancienne cle prive tente
        // de signer avec une cle quelconque -- ne doit jamais verifier
        // contre la VRAIE ancienne cle du registre.
        let fake_rotation_sig = impostor_key.sign(&message);
        let payload = mk_payload(
            &[("public_key", &new_pubkey_hex)],
            &[("rotation_signature", &hex::encode(fake_rotation_sig.to_bytes()))],
        );

        assert_eq!(
            check_rotation_signature(&payload, &real_old_pubkey_hex),
            SignatureCheck::Invalid("verification_failed".to_string())
        );
    }

    #[test]
    fn test_rotation_signature_tampered_payload_fails() {
        let mut csprng = rand_core_shim();
        let old_key = SigningKey::generate(&mut csprng);
        let new_key = SigningKey::generate(&mut csprng);
        let old_pubkey_hex = hex::encode(old_key.verifying_key().to_bytes());
        let new_pubkey_hex = hex::encode(new_key.verifying_key().to_bytes());

        let unsigned = mk_payload(&[("public_key", &new_pubkey_hex)], &[]);
        let rotation_sig = old_key.sign(&signing_bytes(&unsigned));
        let mut payload = mk_payload(
            &[("public_key", &new_pubkey_hex)],
            &[("rotation_signature", &hex::encode(rotation_sig.to_bytes()))],
        );
        payload.intent.insert("name".to_string(), "tampered_after_signing".to_string());

        assert_eq!(
            check_rotation_signature(&payload, &old_pubkey_hex),
            SignatureCheck::Invalid("verification_failed".to_string())
        );
    }

    // ed25519-dalek 2.x attend un rand_core::CryptoRngCore -- rand 0.8's
    // OsRng l'implemente directement, pas besoin d'une dependance de plus.
    fn rand_core_shim() -> rand::rngs::OsRng {
        rand::rngs::OsRng
    }
}
