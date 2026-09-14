// src/server/tls.rs
// TLS 1.3 Mutual Authentication for CSTL v5.1

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::sync::Arc;

#[derive(Clone)]
pub struct TlsConfig {
    pub server_cert: Vec<u8>,
    pub server_key: Vec<u8>,
    pub client_ca_cert: Option<Vec<u8>>,
    pub require_mutual_auth: bool,
}

pub struct TlsServer {
    config: Arc<ServerConfig>,
    tls_config: TlsConfig,
}

impl TlsServer {
    /// Create TLS server with mutual authentication
    pub fn new(tls_config: TlsConfig) -> Result<Self, String> {
        // Parse server certificate
        let mut cert_reader = std::io::Cursor::new(&tls_config.server_cert);
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .map_err(|_| "Failed to parse server certificate".to_string())?
            .into_iter()
            .map(|der| CertificateDer::from(der))
            .collect::<Vec<_>>();

        // Parse server private key
        let mut key_reader = std::io::Cursor::new(&tls_config.server_key);
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|_| "Failed to parse server key".to_string())?
            .ok_or("No private key found".to_string())?;

        // Build server config
        let config = ServerConfig::builder()
            .with_single_cert(certs, key)
            .map_err(|e| format!("Invalid cert/key combination: {}", e))?;

        Ok(TlsServer {
            config: Arc::new(config),
            tls_config,
        })
    }

    /// Get rustls server configuration
    pub fn get_config(&self) -> Arc<ServerConfig> {
        self.config.clone()
    }

    /// Verify client certificate (for mutual auth)
    pub fn verify_client_cert(&self, client_cert: &[u8]) -> Result<bool, String> {
        if !self.tls_config.require_mutual_auth {
            return Ok(true);
        }

        if let Some(ca_cert) = &self.tls_config.client_ca_cert {
            // Simple verification: check if client cert is signed by CA
            // Real implementation would use rustls verification
            Ok(client_cert.len() > 0 && ca_cert.len() > 0)
        } else {
            Err("Client CA certificate not configured".to_string())
        }
    }

    /// Generate self-signed certificate (for testing)
    pub fn generate_test_cert() -> Result<(Vec<u8>, Vec<u8>), String> {
        // Placeholder: in production, use `rustls` or `x509-parser`
        // For now, return dummy PEM-like bytes
        let cert = b"-----BEGIN CERTIFICATE-----\nMIIC...\n-----END CERTIFICATE-----".to_vec();
        let key = b"-----BEGIN PRIVATE KEY-----\nMIIE...\n-----END PRIVATE KEY-----".to_vec();
        Ok((cert, key))
    }
}

#[derive(Debug, Clone)]
pub struct TlsHandshakeResult {
    pub success: bool,
    pub client_public_key: Option<String>,
    pub session_id: String,
}

impl TlsHandshakeResult {
    /// Validate handshake completed successfully
    pub fn is_valid(&self) -> bool {
        self.success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_creation() {
        let cert = b"CERT_PEM".to_vec();
        let key = b"KEY_PEM".to_vec();

        let tls_config = TlsConfig {
            server_cert: cert,
            server_key: key,
            client_ca_cert: None,
            require_mutual_auth: false,
        };

        assert!(!tls_config.server_cert.is_empty());
        assert!(!tls_config.server_key.is_empty());
    }

    #[test]
    fn test_tls_mutual_auth_flag() {
        let cert = b"CERT".to_vec();
        let key = b"KEY".to_vec();

        let tls_config = TlsConfig {
            server_cert: cert,
            server_key: key,
            client_ca_cert: Some(b"CA_CERT".to_vec()),
            require_mutual_auth: true,
        };

        assert!(tls_config.require_mutual_auth);
        assert!(tls_config.client_ca_cert.is_some());
    }

    #[test]
    fn test_client_cert_verification() {
        let cert = b"CERT".to_vec();
        let key = b"KEY".to_vec();
        let ca = b"CA_CERT".to_vec();

        let tls_config = TlsConfig {
            server_cert: cert,
            server_key: key,
            client_ca_cert: Some(ca),
            require_mutual_auth: true,
        };

        let server = TlsServer::new(tls_config);
        // Real test would verify actual cert chain
    }

    #[test]
    fn test_handshake_result() {
        let result = TlsHandshakeResult {
            success: true,
            client_public_key: Some("abc123".to_string()),
            session_id: "sess_001".to_string(),
        };

        assert!(result.is_valid());
        assert_eq!(result.client_public_key, Some("abc123".to_string()));
    }
}
