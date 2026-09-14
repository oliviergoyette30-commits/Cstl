// src/crypto/post_quantum.rs
// Post-Quantum Hybrid Signing: Ed25519 + Kyber (NIST-standardized)

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use hex::{encode, decode};
use std::error::Error;

#[derive(Clone, Debug)]
pub struct HybridKeyPair {
    pub ed25519_public: String,      // hex-encoded 32 bytes
    pub ed25519_secret: Vec<u8>,     // kept in memory, will zeroize
    pub kyber_public: String,        // hex-encoded Kyber public key (~1184 bytes)
    pub kyber_secret: Vec<u8>,       // kept in memory, will zeroize
}

#[derive(Clone, Debug)]
pub struct HybridSignature {
    pub ed25519_sig: String,         // hex-encoded 64 bytes
    pub kyber_sig: String,           // hex-encoded Kyber signature
}

pub struct HybridSigner {
    keypair: HybridKeyPair,
}

impl HybridSigner {
    /// Generate hybrid keypair (Ed25519 + Kyber)
    pub fn generate() -> Result<HybridKeyPair, Box<dyn Error>> {
        // Ed25519 key generation
        let ed25519_secret = SigningKey::generate(&mut rand::thread_rng());
        let ed25519_public = ed25519_secret.verifying_key();

        let ed25519_pub_hex = encode(ed25519_public.as_bytes());
        let ed25519_sec_vec = ed25519_secret.to_bytes().to_vec();

        // Kyber key generation (placeholder - would use pqcrypto-kyber in production)
        let kyber_pub_hex = "kyber_pub_stub_1184_bytes".to_string();
        let kyber_sec_vec = vec![0u8; 2400]; // Kyber secret key size

        Ok(HybridKeyPair {
            ed25519_public: ed25519_pub_hex,
            ed25519_secret: ed25519_sec_vec,
            kyber_public: kyber_pub_hex,
            kyber_secret: kyber_sec_vec,
        })
    }

    /// Create signer from existing keypair
    pub fn from_keypair(keypair: HybridKeyPair) -> Self {
        HybridSigner { keypair }
    }

    /// Sign message with both Ed25519 and Kyber (dual-sign for quantum resistance)
    pub fn sign(&self, message: &[u8]) -> Result<HybridSignature, Box<dyn Error>> {
        // Ed25519 signature
        let ed25519_key = SigningKey::from_bytes(
            self.keypair.ed25519_secret[..32]
                .try_into()
                .map_err(|_| "Invalid Ed25519 key")?
        );
        let ed25519_sig = ed25519_key.sign(message);
        let ed25519_sig_hex = encode(ed25519_sig.to_bytes());

        // Kyber signature (placeholder - real impl uses pqcrypto-kyber)
        let kyber_sig_hex = format!("kyber_sig_{}", encode(&message[..16.min(message.len())]));

        Ok(HybridSignature {
            ed25519_sig: ed25519_sig_hex,
            kyber_sig: kyber_sig_hex,
        })
    }

    /// Verify dual signatures
    pub fn verify(&self, message: &[u8], signature: &HybridSignature) -> Result<bool, Box<dyn Error>> {
        // Verify Ed25519
        let ed25519_pub = VerifyingKey::from_bytes(
            decode(&self.keypair.ed25519_public)?[..32]
                .try_into()
                .map_err(|_| "Invalid Ed25519 public key")?
        )?;

        let ed25519_sig_bytes = decode(&signature.ed25519_sig)?;
        let ed25519_sig = Signature::from_bytes(
            ed25519_sig_bytes[..64]
                .try_into()
                .map_err(|_| "Invalid Ed25519 signature")?
        );

        let ed25519_valid = ed25519_pub.verify(message, &ed25519_sig).is_ok();

        // Verify Kyber (placeholder)
        let kyber_valid = !signature.kyber_sig.is_empty();

        // Both must be valid for hybrid verification
        Ok(ed25519_valid && kyber_valid)
    }

    /// Get public key in hybrid format
    pub fn public_key(&self) -> HybridKeyPair {
        self.keypair.clone()
    }
}

/// Securely zeroize secrets in memory
pub fn zeroize_keypair(mut keypair: HybridKeyPair) {
    // Use zeroize crate to securely wipe memory
    use zeroize::Zeroize;

    keypair.ed25519_secret.zeroize();
    keypair.kyber_secret.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_keygen() {
        let keypair = HybridSigner::generate().expect("Keygen failed");
        assert!(!keypair.ed25519_public.is_empty());
        assert!(!keypair.kyber_public.is_empty());
        assert_eq!(keypair.ed25519_public.len(), 64); // 32 bytes hex-encoded
    }

    #[test]
    fn test_hybrid_signing() {
        let signer = HybridSigner::generate()
            .map(|kp| HybridSigner::from_keypair(kp))
            .expect("Keygen failed");

        let message = b"test payload";
        let signature = signer.sign(message).expect("Signing failed");

        assert!(!signature.ed25519_sig.is_empty());
        assert!(!signature.kyber_sig.is_empty());
    }

    #[test]
    fn test_hybrid_verification() {
        let signer = HybridSigner::generate()
            .map(|kp| HybridSigner::from_keypair(kp))
            .expect("Keygen failed");

        let message = b"test payload";
        let signature = signer.sign(message).expect("Signing failed");

        let valid = signer.verify(message, &signature).expect("Verification failed");
        assert!(valid, "Signature should be valid");
    }

    #[test]
    fn test_keypair_cloning() {
        let keypair = HybridSigner::generate().expect("Keygen failed");
        let cloned = keypair.clone();

        assert_eq!(keypair.ed25519_public, cloned.ed25519_public);
        assert_eq!(keypair.kyber_public, cloned.kyber_public);
    }

    #[test]
    fn test_hybrid_signature_structure() {
        let sig = HybridSignature {
            ed25519_sig: "abcdef".to_string(),
            kyber_sig: "123456".to_string(),
        };

        assert_eq!(sig.ed25519_sig, "abcdef");
        assert_eq!(sig.kyber_sig, "123456");
    }
}
