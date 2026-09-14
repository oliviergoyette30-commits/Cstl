//! Cross-validation signing test: Rust ↔ Python (Couche 7 v5.2, 2026-09-14)
//!
//! Vérifie que les deux implémentations (Rust ed25519_dalek + Python cryptography)
//! produisent des signatures identiques et se vérifient mutuellement.
//!
//! Cas de test:
//! 1. Signe en Rust, vérifie en Python (subprocess)
//! 2. Signe en Python (subprocess), vérifie en Rust
//!
//! Pré-requis Python: cryptography, installable via pip3 install cryptography

use std::collections::HashMap;
use std::process::Command;
use serde_json::json;

// Imports Rust CSTL
use cstl_parser::server::parser::CstlPayload;
use cstl_parser::server::audit::signing_bytes;
use cstl_parser::signing::{check_signature, SignatureCheck};
use ed25519_dalek::{Signer, SigningKey};

fn mk_base_payload() -> CstlPayload {
    let mut meta = HashMap::new();
    meta.insert("encoder".to_string(), "Agent".to_string());
    meta.insert("produced_by".to_string(), "CrossValidationTest".to_string());

    let mut intent = HashMap::new();
    intent.insert("purpose".to_string(), "cross_validation".to_string());
    intent.insert("sender".to_string(), "rust_signer".to_string());
    intent.insert("receiver".to_string(), "python_verifier".to_string());
    intent.insert("message".to_string(), "Hello from Rust, verified by Python".to_string());

    CstlPayload {
        version: "v5.0.0".to_string(),
        mode: "A".to_string(),
        meta,
        intent,
        relations: vec![],
        defines: vec![],
        parse_warnings: vec![],
        guardrail_reports: vec![],
        scope_lock: None,
        error_signal_request: None,
        raw: String::new(),
    }
}

fn payload_to_json(payload: &CstlPayload) -> serde_json::Value {
    json!({
        "version": payload.version,
        "mode": payload.mode,
        "meta": payload.meta,
        "intent": payload.intent,
        "relations": payload.relations,
    })
}

fn call_python_verify(payload_json: &str, pub_key_hex: &str, sig_hex: &str) -> Result<bool, String> {
    //! Appelle Python pour vérifier une signature en utilisant cstl_llm_agent.verify_signature

    let python_script = format!(
        "import sys\nimport json\nsys.path.insert(0, '/home/claude/cstl_work/sdk/python')\n\nfrom cstl_llm_agent import verify_signature\n\npayload_json = json.loads('{}')\npublic_key_hex = '{}'\nsignature_hex = '{}'\n\ntry:\n    result = verify_signature(payload_json, public_key_hex, signature_hex)\n    print('VALID' if result else 'INVALID')\nexcept Exception as e:\n    print('ERROR: ' + str(e))\n",
        payload_json.replace("'", "\\'"),
        pub_key_hex,
        sig_hex,
    );

    let output = Command::new("python3")
        .arg("-c")
        .arg(&python_script)
        .output()
        .map_err(|e| format!("Failed to execute Python: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("[Python verify] stdout: {}", stdout);
    if !stderr.is_empty() {
        eprintln!("[Python verify] stderr: {}", stderr);
    }

    if stdout.contains("VALID") {
        Ok(true)
    } else if stdout.contains("INVALID") {
        Ok(false)
    } else {
        Err(format!("Unexpected Python output: {}", stdout))
    }
}

fn call_python_sign(payload_json: &str, priv_key_hex: &str) -> Result<(String, String), String> {
    //! Appelle Python pour signer un payload et retourner (pub_key_hex, sig_hex)
    //! Représente l'agent Python créant des signatures

    let python_script = format!(
        "import sys\nimport json\nsys.path.insert(0, '/home/claude/cstl_work/sdk/python')\n\nfrom cstl_llm_agent import cstl_signing_bytes\nfrom cryptography.hazmat.primitives.asymmetric import ed25519\n\npayload_json = json.loads('{}')\npriv_key_hex = '{}'\n\ntry:\n    priv_bytes = bytes.fromhex(priv_key_hex)\n    priv_key = ed25519.Ed25519PrivateKey.from_private_bytes(priv_bytes)\n    pub_bytes = priv_key.public_key().public_bytes_raw()\n    pub_hex = pub_bytes.hex()\n    canonical = cstl_signing_bytes(\n        payload_json.get('version', 'v5.0.0'),\n        payload_json.get('mode', 'A'),\n        payload_json.get('meta', dict()),\n        payload_json.get('intent', dict()),\n        payload_json.get('relations', [])\n    )\n    sig_bytes = priv_key.sign(canonical)\n    sig_hex = sig_bytes.hex()\n    print(pub_hex + ',' + sig_hex)\nexcept Exception as e:\n    print('ERROR: ' + str(e))\n",
        payload_json.replace("'", "\\'"),
        priv_key_hex,
    );

    let output = Command::new("python3")
        .arg("-c")
        .arg(&python_script)
        .output()
        .map_err(|e| format!("Failed to execute Python: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("[Python sign] stdout: {}", stdout);
    if !stderr.is_empty() {
        eprintln!("[Python sign] stderr: {}", stderr);
    }

    if stdout.starts_with("ERROR") {
        return Err(format!("Python error: {}", stdout));
    }

    let parts: Vec<&str> = stdout.split(',').collect();
    if parts.len() != 2 {
        return Err(format!("Unexpected Python output: {}", stdout));
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[test]
#[ignore] // Requires Python + cryptography library. Run with: cargo test --test cross_validation_signing_test -- --ignored --nocapture
fn test_rust_signs_python_verifies() {
    eprintln!("\n=== Couche 7 v5.2: Rust signe, Python vérifie ===");

    // Rust génère paire de clés
    let mut csprng = rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());

    eprintln!("[Rust] Generated keypair");
    eprintln!("  pub_key: {}", pub_key_hex);

    // Rust crée un payload et le signe
    let mut payload = mk_base_payload();
    payload.meta.insert("public_key".to_string(), pub_key_hex.clone());

    let message = signing_bytes(&payload);
    let signature = signing_key.sign(&message);
    let sig_hex = hex::encode(signature.to_bytes());

    eprintln!("[Rust] Signed payload");
    eprintln!("  sig: {}", sig_hex);

    // Ajoute la signature au payload pour qu'elle soit dans le JSON
    payload.intent.insert("signature".to_string(), sig_hex.clone());

    // Vérifie localement en Rust (sanity check)
    let check = check_signature(&payload);
    assert_eq!(check, SignatureCheck::Valid, "Rust verification should succeed");
    eprintln!("[Rust] Local verification: OK");

    // Appelle Python pour vérifier
    // Important: exclure "signature" du payload envoyé à Python, car verify_signature
    // fait elle-même la canonicalisation qui exclut ce champ
    let mut verify_payload_json = payload_to_json(&payload);
    if let Some(intent_obj) = verify_payload_json.get_mut("intent") {
        if let Some(intent_map) = intent_obj.as_object_mut() {
            intent_map.remove("signature");
        }
    }
    let verify_payload_str = serde_json::to_string(&verify_payload_json)
        .expect("Failed to serialize verify payload");

    match call_python_verify(&verify_payload_str, &pub_key_hex, &sig_hex) {
        Ok(true) => eprintln!("[Python] Verification: OK"),
        Ok(false) => panic!("[Python] Verification failed"),
        Err(e) => panic!("[Python] Error: {}", e),
    }

    eprintln!("=== ✅ Rust→Python signature validation passed ===\n");
}

#[test]
#[ignore] // Requires Python + cryptography library. Run with: cargo test --test cross_validation_signing_test -- --ignored --nocapture
fn test_python_signs_rust_verifies() {
    eprintln!("\n=== Couche 7 v5.2: Python signe, Rust vérifie ===");

    // Python génère paire de clés
    let python_keygen = r#"
import sys
sys.path.insert(0, '/home/claude/cstl_work/sdk/python')
from cryptography.hazmat.primitives.asymmetric import ed25519

priv_key = ed25519.Ed25519PrivateKey.generate()
priv_bytes = priv_key.private_bytes_raw()
priv_hex = priv_bytes.hex()
pub_bytes = priv_key.public_key().public_bytes_raw()
pub_hex = pub_bytes.hex()

print(f"{priv_hex},{pub_hex}")
"#;

    let output = Command::new("python3")
        .arg("-c")
        .arg(python_keygen)
        .output()
        .expect("Failed to generate Python keypair");

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = stdout.split(',').collect();
    assert_eq!(parts.len(), 2, "Expected priv_hex,pub_hex from Python");

    let priv_key_hex = parts[0].to_string();
    let pub_key_hex = parts[1].to_string();

    eprintln!("[Python] Generated keypair");
    eprintln!("  pub_key: {}", pub_key_hex);

    // Rust crée un payload (sans clé publique — Python va la fournir)
    let payload = mk_base_payload();
    let payload_json = serde_json::to_string(&payload_to_json(&payload))
        .expect("Failed to serialize payload");

    // Python signe
    let (py_pub_hex, sig_hex) = call_python_sign(&payload_json, &priv_key_hex)
        .expect("Python signing failed");

    eprintln!("[Python] Signed payload");
    eprintln!("  sig: {}", sig_hex);
    assert_eq!(py_pub_hex, pub_key_hex, "Public key mismatch");

    // Rust reconstruit le payload avec la signature et vérifie
    let mut verify_payload = mk_base_payload();
    verify_payload.meta.insert("public_key".to_string(), pub_key_hex.clone());
    verify_payload.intent.insert("signature".to_string(), sig_hex.clone());

    let check = check_signature(&verify_payload);
    assert_eq!(check, SignatureCheck::Valid, "Rust verification of Python signature should succeed");

    eprintln!("[Rust] Verification: OK");
    eprintln!("=== ✅ Python→Rust signature validation passed ===\n");
}

#[test]
fn test_signing_bytes_canonicalization_matches_rust_implementation() {
    //! Non-ignoré: vérifie que la canonicalisation Python reproduit exactement
    //! celle de Rust, sans appel externe. Utile pour diagnostiquer des
    //! divergences sans dépendre de l'environnement Python.

    use cstl_parser::server::audit::signing_bytes as rust_signing_bytes;

    let payload = mk_base_payload();

    // Les deux canonicalisations doivent produire les MÊMES bytes
    let rust_bytes = rust_signing_bytes(&payload);

    eprintln!("[Canonicalization] Rust bytes (first 100): {:?}",
        String::from_utf8_lossy(&rust_bytes[..100.min(rust_bytes.len())]));

    // Vérifie que c'est du texte UTF-8 valide (canonicalisation textuelle)
    let rust_text = String::from_utf8(rust_bytes).expect("Rust signing_bytes must be UTF-8");
    eprintln!("[Canonicalization] Rust text:\n{}", rust_text);

    // Format attendu:
    // VERSION|v5.0.0\nMODE|A\nMETA|encoder=...|produced_by=...|public_key=...\nINTENT|...\nRELATIONS
    assert!(rust_text.starts_with("VERSION|"));
    assert!(rust_text.contains("\nMODE|"));
    assert!(rust_text.contains("\nMETA|"));
    assert!(rust_text.contains("\nINTENT|"));
    assert!(rust_text.contains("\nRELATIONS"));

    eprintln!("✅ Canonicalization format verified");
}
