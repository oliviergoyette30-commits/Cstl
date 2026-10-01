//! src/server/tls.rs -- TLS 1.3 avec authentification mutuelle optionnelle
//! (Couche 2, securite transport). Implementation REELLE (2026-10-01),
//! remplace un stub trouve par le meme grep `pub mod X` / zero-reference
//! externe que CASTLE et quorum.rs plus tot cette session -- a une
//! difference pres, importante: le stub ne faisait AUCUNE vraie crypto.
//! `verify_client_cert` acceptait n'importe quel octet non vide comme
//! certificat valide (`client_cert.len() > 0 && ca_cert.len() > 0`), et
//! `generate_test_cert` retournait du texte PEM factice tronque
//! (`"MIIC...\n"`) qui ne parserait meme pas. Le brancher tel quel aurait
//! fait croire que le serveur offre une authentification mutuelle TLS 1.3
//! reelle sans aucune verification cryptographique -- security theater, pas
//! une feature incomplete. Reecrit entierement avec `rustls` 0.23 (deja une
//! dependance du Cargo.toml depuis "v5.1 Dependencies", jamais utilisee
//! nulle part avant ce commit) pour le parsing de certificats X.509 reel, la
//! verification de chaine reelle (`WebPkiClientVerifier`), et un vrai
//! handshake TLS 1.3 -- cable dans `listener.rs`/`handler.rs` (voir ces
//! fichiers pour le branchement reseau), pas seulement une bibliotheque
//! isolee comme avant.
//!
//! Opt-in, backward compatible: si `CSTL_TLS_CERT_PATH`/`CSTL_TLS_KEY_PATH`
//! ne sont pas definies au demarrage, le serveur continue en TCP brut
//! (comportement inchange depuis le debut de la session) -- meme discipline
//! que `CSTL_COLLECT_RESPONSE_CORPUS`/`castle_response`: une nouvelle
//! capacite ne doit jamais changer le comportement par defaut.

use std::io::Cursor;
use std::sync::{Arc, Once};

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

/// `ServerConfig::builder()` (l'API "securisee par defaut" de rustls 0.23)
/// panique si aucun `CryptoProvider` n'a ete installe pour le processus.
/// Installe le fournisseur `aws-lc-rs` -- feature par defaut de la
/// dependance `rustls` dans ce Cargo.toml (`default = ["aws_lc_rs", ...]`,
/// pas "ring": verifie en inspectant `rustls`'s propre Cargo.toml avant
/// d'ecrire ce code, pas suppose) -- UNE SEULE FOIS par processus.
/// `install_default` retourne une erreur si un provider est deja installe --
/// ignoree volontairement: un provider deja present (ex. un deuxieme
/// `TlsServer::new` dans le meme process, frequent dans les tests) est
/// exactement l'etat voulu, pas un echec.
static CRYPTO_PROVIDER_INIT: Once = Once::new();
fn ensure_crypto_provider_installed() {
    CRYPTO_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Octets PEM bruts -- charges depuis le disque par l'appelant (voir
/// `server/mod.rs::start`, variables d'environnement `CSTL_TLS_*`). Garder
/// ce type ignorant de la source (fichier/env/autre) rend `TlsServer::new`
/// testable sans toucher au disque.
#[derive(Clone)]
pub struct TlsConfig {
    /// Chaine de certificats du serveur (PEM), feuille en premier.
    pub cert_chain_pem: Vec<u8>,
    /// Cle privee du serveur (PEM), correspondant au premier certificat de
    /// `cert_chain_pem`.
    pub private_key_pem: Vec<u8>,
    /// Certificat(s) CA de confiance (PEM) pour verifier les certificats
    /// CLIENT -- requis si `require_mutual_auth=true`, ignore sinon.
    pub client_ca_pem: Option<Vec<u8>>,
    /// Si vrai, un client DOIT presenter un certificat signe par
    /// `client_ca_pem` pour completer le handshake -- rustls rejette la
    /// connexion AU NIVEAU TLS (avant que le moindre octet applicatif ne
    /// soit lu), pas une verification applicative apres coup comme le
    /// faisait l'ancien stub.
    pub require_mutual_auth: bool,
}

/// Enveloppe un `Arc<rustls::ServerConfig>` deja construit et valide --
/// `TlsServer::new` fait tout le travail de parsing/validation une seule
/// fois au demarrage; `acceptor()` est ensuite appele par connexion (bon
/// marche, clone juste l'Arc).
pub struct TlsServer {
    rustls_config: Arc<ServerConfig>,
}

impl TlsServer {
    /// Construit la configuration TLS serveur a partir de PEM reels.
    /// Echoue (sans paniquer) si: certificat/cle serveur absents ou
    /// malformes, `require_mutual_auth=true` sans `client_ca_pem`, CA client
    /// invalide, ou cle privee incompatible avec le certificat (rustls le
    /// detecte a `with_single_cert`).
    pub fn new(tls_config: TlsConfig) -> Result<Self, String> {
        ensure_crypto_provider_installed();

        if tls_config.cert_chain_pem.is_empty() {
            return Err("certificat serveur vide".to_string());
        }
        if tls_config.private_key_pem.is_empty() {
            return Err("cle privee serveur vide".to_string());
        }
        if tls_config.require_mutual_auth && tls_config.client_ca_pem.is_none() {
            return Err("require_mutual_auth=true exige client_ca_pem".to_string());
        }

        let cert_chain = parse_cert_chain(&tls_config.cert_chain_pem)?;
        if cert_chain.is_empty() {
            return Err("aucun certificat trouve dans cert_chain_pem".to_string());
        }
        let private_key = parse_private_key(&tls_config.private_key_pem)?;

        let builder = ServerConfig::builder();
        let builder = if tls_config.require_mutual_auth {
            // unwrap() sur: deja valide ci-dessus (require_mutual_auth=true
            // => client_ca_pem.is_some()).
            let ca_pem = tls_config.client_ca_pem.as_ref().unwrap();
            let ca_certs = parse_cert_chain(ca_pem)?;
            if ca_certs.is_empty() {
                return Err("aucun certificat trouve dans client_ca_pem".to_string());
            }
            let mut roots = RootCertStore::empty();
            for cert in ca_certs {
                roots
                    .add(cert)
                    .map_err(|e| format!("certificat CA client invalide: {e}"))?;
            }
            let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| format!("construction du verificateur de certificat client echouee: {e}"))?;
            builder.with_client_cert_verifier(verifier)
        } else {
            builder.with_no_client_auth()
        };

        let config = builder
            .with_single_cert(cert_chain, private_key)
            .map_err(|e| format!("certificat/cle serveur invalides (chaine et cle doivent correspondre): {e}"))?;

        Ok(TlsServer {
            rustls_config: Arc::new(config),
        })
    }

    /// Cree un `tokio_rustls::TlsAcceptor` pret a envelopper un
    /// `TcpStream` accepte par le listener (voir `listener.rs`). Bon marche
    /// -- `TlsAcceptor::from` ne fait que cloner l'`Arc<ServerConfig>`.
    pub fn acceptor(&self) -> tokio_rustls::TlsAcceptor {
        tokio_rustls::TlsAcceptor::from(self.rustls_config.clone())
    }
}

fn parse_cert_chain(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    let mut reader = Cursor::new(pem);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parsing PEM certificat echoue: {e}"))
}

fn parse_private_key(pem: &[u8]) -> Result<PrivateKeyDer<'static>, String> {
    let mut reader = Cursor::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| format!("parsing PEM cle privee echoue: {e}"))?
        .ok_or_else(|| "aucune cle privee trouvee dans le PEM fourni".to_string())
}

// ============================================================================
// Generation de certificats de TEST (CA + serveur + client), REELS -- 2026-10-01
// ============================================================================
//
// L'ancien `generate_test_cert()` du stub retournait un PEM factice qui ne
// parserait meme pas. Pour verifier reellement un handshake TLS mutuel (voir
// `examples/tls_smoke_test.rs`), il faut une vraie petite chaine de
// confiance: une CA auto-signee qui signe a la fois un certificat serveur et
// un certificat client -- pas juste UN certificat auto-signe isole (un
// handshake mutuel exige que le verificateur de certificat client ait une CA
// a laquelle comparer, voir `TlsConfig::client_ca_pem` ci-dessus).
//
// JAMAIS a utiliser en production: la cle privee de la CA n'est conservee
// nulle part apres l'appel (aucune rotation, aucune revocation possible), et
// les certificats generes n'ont aucune duree de vie realiste pensee pour un
// deploiement reel.

/// Chaine de confiance jetable complete: CA + certificat/cle serveur +
/// certificat/cle client, tous en PEM.
pub struct TestPki {
    pub ca_cert_pem: Vec<u8>,
    pub server_cert_pem: Vec<u8>,
    pub server_key_pem: Vec<u8>,
    pub client_cert_pem: Vec<u8>,
    pub client_key_pem: Vec<u8>,
}

/// Genere une CA auto-signee, puis un certificat serveur (valide pour
/// `server_names`, ex. `["localhost", "127.0.0.1"]`) et un certificat
/// client (identifie par `client_common_name`), tous deux signes par cette
/// CA. `TestPki::ca_cert_pem` est ce qu'on met dans
/// `TlsConfig::client_ca_pem` pour accepter `client_cert_pem`/`client_key_pem`
/// cote client ET pour que le CLIENT verifie `server_cert_pem` a son tour
/// (meme CA, confiance mutuelle reelle -- pas juste serveur->client).
pub fn generate_test_pki(server_names: Vec<String>, client_common_name: &str) -> Result<TestPki, String> {
    use rcgen::{BasicConstraints, CertificateParams, Issuer, IsCa, KeyPair};

    let ca_key = KeyPair::generate().map_err(|e| format!("generation de la cle CA echouee: {e}"))?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| format!("parametres CA invalides: {e}"))?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .map_err(|e| format!("auto-signature de la CA echouee: {e}"))?;
    let issuer = Issuer::from_params(&ca_params, &ca_key);

    let server_key = KeyPair::generate().map_err(|e| format!("generation de la cle serveur echouee: {e}"))?;
    let server_params = CertificateParams::new(server_names)
        .map_err(|e| format!("parametres de certificat serveur invalides: {e}"))?;
    let server_cert = server_params
        .signed_by(&server_key, &issuer)
        .map_err(|e| format!("signature du certificat serveur echouee: {e}"))?;

    let client_key = KeyPair::generate().map_err(|e| format!("generation de la cle client echouee: {e}"))?;
    let mut client_params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| format!("parametres de certificat client invalides: {e}"))?;
    client_params.distinguished_name.push(rcgen::DnType::CommonName, client_common_name);
    let client_cert = client_params
        .signed_by(&client_key, &issuer)
        .map_err(|e| format!("signature du certificat client echouee: {e}"))?;

    Ok(TestPki {
        ca_cert_pem: ca_cert.pem().into_bytes(),
        server_cert_pem: server_cert.pem().into_bytes(),
        server_key_pem: server_key.serialize_pem().into_bytes(),
        client_cert_pem: client_cert.pem().into_bytes(),
        client_key_pem: client_key.serialize_pem().into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_test_pki_produces_parseable_real_certs() {
        // La trouvaille motivant toute cette reecriture: l'ancien
        // generate_test_cert() retournait du PEM qui ne PARSAIT meme pas.
        // Ce test echouerait immediatement sur l'ancien stub.
        let pki = generate_test_pki(vec!["localhost".to_string()], "test-client")
            .expect("generation de la PKI de test doit reussir");
        assert!(!parse_cert_chain(&pki.ca_cert_pem).unwrap().is_empty());
        assert!(!parse_cert_chain(&pki.server_cert_pem).unwrap().is_empty());
        assert!(!parse_cert_chain(&pki.client_cert_pem).unwrap().is_empty());
        parse_private_key(&pki.server_key_pem).expect("cle serveur doit parser");
        parse_private_key(&pki.client_key_pem).expect("cle client doit parser");
    }

    #[test]
    fn test_tls_server_rejects_empty_cert() {
        let config = TlsConfig {
            cert_chain_pem: vec![],
            private_key_pem: b"x".to_vec(),
            client_ca_pem: None,
            require_mutual_auth: false,
        };
        let err = match TlsServer::new(config) {
            Err(e) => e,
            Ok(_) => panic!("certificat vide doit etre rejete"),
        };
        assert!(err.contains("certificat serveur vide"), "message inattendu: {err}");
    }

    #[test]
    fn test_tls_server_rejects_mutual_auth_without_ca() {
        let pki = generate_test_pki(vec!["localhost".to_string()], "c").unwrap();
        let config = TlsConfig {
            cert_chain_pem: pki.server_cert_pem,
            private_key_pem: pki.server_key_pem,
            client_ca_pem: None,
            require_mutual_auth: true,
        };
        let err = match TlsServer::new(config) {
            Err(e) => e,
            Ok(_) => panic!("mutual auth sans CA doit etre rejete"),
        };
        assert!(err.contains("require_mutual_auth=true exige client_ca_pem"), "message inattendu: {err}");
    }

    #[test]
    fn test_tls_server_rejects_garbage_pem() {
        // Preuve directe que le parsing est reel: du texte qui n'est PAS du
        // PEM valide (contrairement a l'ancien "MIIC...\n" qui n'etait
        // jamais effectivement parse par le stub) doit etre rejete ici.
        let config = TlsConfig {
            cert_chain_pem: b"ceci n'est pas un certificat".to_vec(),
            private_key_pem: b"ceci n'est pas une cle".to_vec(),
            client_ca_pem: None,
            require_mutual_auth: false,
        };
        assert!(TlsServer::new(config).is_err(), "du PEM invalide doit etre rejete, pas accepte silencieusement");
    }

    #[test]
    fn test_tls_server_accepts_real_generated_cert_without_mutual_auth() {
        let pki = generate_test_pki(vec!["localhost".to_string(), "127.0.0.1".to_string()], "c").unwrap();
        let config = TlsConfig {
            cert_chain_pem: pki.server_cert_pem,
            private_key_pem: pki.server_key_pem,
            client_ca_pem: None,
            require_mutual_auth: false,
        };
        TlsServer::new(config).expect("un certificat/cle serveur reels et valides doivent etre acceptes");
    }

    #[test]
    fn test_tls_server_accepts_real_generated_chain_with_mutual_auth() {
        let pki = generate_test_pki(vec!["localhost".to_string()], "test-client").unwrap();
        let config = TlsConfig {
            cert_chain_pem: pki.server_cert_pem,
            private_key_pem: pki.server_key_pem,
            client_ca_pem: Some(pki.ca_cert_pem),
            require_mutual_auth: true,
        };
        TlsServer::new(config).expect("mutual auth avec une CA reelle et valide doit reussir");
    }

    #[test]
    fn test_tls_server_rejects_mismatched_key() {
        // Cle d'un certificat different de celle du certificat serveur --
        // rustls doit le detecter a with_single_cert (cle publique du
        // certificat != cle derivee de la cle privee), meme sans savoir que
        // "mismatched" signifie quoi que ce soit a notre niveau: c'est
        // rustls qui porte cette garantie, pas du code ecrit ici.
        let pki_a = generate_test_pki(vec!["localhost".to_string()], "a").unwrap();
        let pki_b = generate_test_pki(vec!["localhost".to_string()], "b").unwrap();
        let config = TlsConfig {
            cert_chain_pem: pki_a.server_cert_pem,
            private_key_pem: pki_b.server_key_pem, // cle de la MAUVAISE PKI
            client_ca_pem: None,
            require_mutual_auth: false,
        };
        assert!(TlsServer::new(config).is_err(), "cle privee ne correspondant pas au certificat doit etre rejetee");
    }
}
