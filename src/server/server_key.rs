//! src/server/server_key.rs -- cle Ed25519 PROPRE au serveur, et jetons
//! d'autorisation signes (couche 9 obligatoire, 2026-11-20).
//!
//! Probleme : la couche 9 repondait `decision=allowed`, mais rien n'obligeait
//! un agent a demander, ni le systeme vise (base de donnees, API) a verifier
//! qu'il avait demande. Un agent pouvait agir sans autorisation, ou
//! pretendre en avoir une.
//!
//! Ici, chaque decision `allowed` porte un JETON signe par la cle du serveur :
//! qui (agent), quoi (action), une fois (request_id), quand (issued_at,
//! expires_at) et la trace d'audit (audit_hash). Le systeme vise connait la
//! cle PUBLIQUE du serveur (fichier `<cle>.pub`, distribue hors bande) et
//! refuse toute action sans jeton valide : c'est lui qui rend le controle
//! obligatoire, pas l'agent. Verificateurs : `verify_authorization` (Rust)
//! et `sdk/python/cstl_guard.py` (Python), sur le MEME message canonique.
//!
//! Emplacement de la cle : `CSTL_SERVER_KEY_PATH`, sinon `<base>.server_key`
//! a cote de la base SQLite ; base `:memory:` -> cle ephemere en memoire.
//! Fichier : la graine (32 octets) en hexadecimal, droits 0600.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

pub const AUTHZ_VERSION: &str = "cstl-authz-v1";
pub const AUDIT_VERSION: &str = "cstl-audit-v1";
const SINCE_VERSION: &str = "cstl-audit-since-v1";

pub struct ServerKey {
    signing: SigningKey,
    pub path: Option<PathBuf>,
}

impl ServerKey {
    pub fn ephemeral() -> Self {
        ServerKey { signing: SigningKey::generate(&mut rand::rngs::OsRng), path: None }
    }

    /// Cle a cote de la base (`data_path`), ou `CSTL_SERVER_KEY_PATH`.
    pub fn for_data_path(data_path: &str) -> Result<Self, String> {
        match std::env::var("CSTL_SERVER_KEY_PATH") {
            Ok(p) if !p.is_empty() => Self::load_or_create(Path::new(&p)),
            _ if data_path == ":memory:" => Ok(Self::ephemeral()),
            _ => Self::load_or_create(Path::new(&format!("{data_path}.server_key"))),
        }
    }

    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        let signing = if path.exists() {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cle serveur illisible '{}': {e}", path.display()))?;
            let bytes = hex::decode(text.trim())
                .map_err(|e| format!("cle serveur invalide '{}': {e}", path.display()))?;
            let seed: [u8; 32] = bytes
                .try_into()
                .map_err(|_| format!("cle serveur invalide '{}': 32 octets attendus", path.display()))?;
            SigningKey::from_bytes(&seed)
        } else {
            let key = SigningKey::generate(&mut rand::rngs::OsRng);
            write_private(path, &hex::encode(key.to_bytes()))?;
            key
        };
        let pub_path = public_key_path(path);
        let public = hex::encode(signing.verifying_key().to_bytes());
        std::fs::write(&pub_path, format!("{public}\n"))
            .map_err(|e| format!("ecriture de '{}' impossible: {e}", pub_path.display()))?;
        Ok(ServerKey { signing, path: Some(path.to_path_buf()) })
    }

    pub fn public_hex(&self) -> String {
        hex::encode(self.signing.verifying_key().to_bytes())
    }

    pub fn sign_hex(&self, message: &[u8]) -> String {
        hex::encode(self.signing.sign(message).to_bytes())
    }

    pub fn verify_hex(&self, message: &[u8], signature_hex: &str) -> bool {
        let Some(bytes) = hex::decode(signature_hex).ok().and_then(|b| <[u8; 64]>::try_from(b).ok()) else {
            return false;
        };
        self.signing.verifying_key().verify(message, &Signature::from_bytes(&bytes)).is_ok()
    }

    /// Premier `seq` de la chaine d'audit qui DOIT etre signe (2026-11-20).
    /// Les maillons plus anciens datent d'avant la signature : ils restent
    /// verifies par leurs liens et leur contenu, pas par une signature.
    ///
    /// Stocke a cote de la cle (`<cle>.audit_since`), SIGNE : sans ca, un
    /// attaquant qui reecrit la base pourrait retirer toutes les signatures
    /// et faire passer tout l'historique pour "ancien". Absent -> cree avec
    /// `chain_len` (tout maillon a venir sera signe). Present mais signature
    /// invalide -> erreur (marqueur falsifie). Cle ephemere -> `chain_len`.
    pub fn audit_signed_since(&self, chain_len: u64) -> Result<u64, String> {
        let Some(key_path) = &self.path else { return Ok(chain_len) };
        let path = audit_since_path(key_path);
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("marqueur '{}' illisible: {e}", path.display()))?;
            let mut since = None;
            let mut sig = None;
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("since=") { since = v.trim().parse::<u64>().ok(); }
                if let Some(v) = line.strip_prefix("signature=") { sig = Some(v.trim().to_string()); }
            }
            return match (since, sig) {
                (Some(n), Some(sig)) if self.verify_hex(since_message(n).as_bytes(), &sig) => Ok(n),
                _ => Err(format!(
                    "marqueur '{}' falsifie ou illisible : impossible de savoir depuis quand la chaine d'audit est signee",
                    path.display()
                )),
            };
        }
        let sig = self.sign_hex(since_message(chain_len).as_bytes());
        std::fs::write(&path, format!("since={chain_len}\nsignature={sig}\n"))
            .map_err(|e| format!("ecriture de '{}' impossible: {e}", path.display()))?;
        Ok(chain_len)
    }
}

fn since_message(n: u64) -> String {
    format!("{SINCE_VERSION}\nsince={n}")
}

pub fn audit_since_path(key_path: &Path) -> PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".audit_since");
    PathBuf::from(p)
}

/// Message canonique signe pour un maillon de la chaine d'audit : tous les
/// champs persistes dans `audit_trail`. Modifier le contenu d'un message
/// change son hash, donc ce message : la signature ne tient plus, meme si
/// l'attaquant recalcule tous les hash.
pub fn audit_message(seq: u64, hash: &str, parent_hash: &str, sender: &str, receiver: &str, purpose: &str) -> String {
    format!("{AUDIT_VERSION}\nseq={seq}\nhash={hash}\nparent_hash={parent_hash}\nsender={sender}\nreceiver={receiver}\npurpose={purpose}")
}

pub fn public_key_path(key_path: &Path) -> PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".pub");
    PathBuf::from(p)
}

fn write_private(path: &Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| format!("creation de la cle serveur '{}' impossible: {e}", path.display()))?;
    f.write_all(content.as_bytes())
        .map_err(|e| format!("ecriture de la cle serveur '{}' impossible: {e}", path.display()))
}

/// Contenu d'un jeton d'autorisation (tout ce que la signature couvre).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub agent: String,
    pub action: String,
    pub request_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub audit_hash: String,
}

impl Authorization {
    /// Message canonique signe. Une ligne par champ, ordre fixe : aucun
    /// champ ne peut contenir de saut de ligne (valeurs du wire CSTL).
    pub fn message(&self) -> String {
        format!(
            "{AUTHZ_VERSION}\nagent={}\naction={}\nrequest_id={}\nissued_at={}\nexpires_at={}\naudit_hash={}",
            self.agent, self.action, self.request_id, self.issued_at, self.expires_at, self.audit_hash
        )
    }
}

/// Pourquoi un jeton est refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthzError {
    BadKey,
    BadSignature,
    WrongAction,
    WrongAgent,
    Expired,
    NotYetValid,
}

/// Verification cote systeme vise. `now` : secondes unix ; `skew` : tolerance
/// d'horloge. Ne verifie PAS le rejeu : au systeme vise de refuser un
/// `request_id` deja utilise (voir `cstl_guard.py`).
pub fn verify_authorization(
    server_public_hex: &str,
    auth: &Authorization,
    signature_hex: &str,
    expected_action: &str,
    expected_agent: Option<&str>,
    now: u64,
    skew: u64,
) -> Result<(), AuthzError> {
    let key_bytes: [u8; 32] = hex::decode(server_public_hex)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(AuthzError::BadKey)?;
    let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| AuthzError::BadKey)?;
    let sig_bytes: [u8; 64] = hex::decode(signature_hex)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(AuthzError::BadSignature)?;
    key.verify(auth.message().as_bytes(), &Signature::from_bytes(&sig_bytes))
        .map_err(|_| AuthzError::BadSignature)?;
    if auth.action != expected_action {
        return Err(AuthzError::WrongAction);
    }
    if expected_agent.is_some_and(|a| a != auth.agent) {
        return Err(AuthzError::WrongAgent);
    }
    if auth.issued_at > now + skew {
        return Err(AuthzError::NotYetValid);
    }
    if now > auth.expires_at + skew {
        return Err(AuthzError::Expired);
    }
    Ok(())
}

/// Duree de validite d'un jeton (`CSTL_AUTHZ_TTL_SECS`, defaut 60 s).
pub fn ttl_from_env() -> u64 {
    std::env::var("CSTL_AUTHZ_TTL_SECS").ok().and_then(|v| v.parse().ok()).filter(|&t| t > 0).unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Authorization {
        Authorization {
            agent: "bot".into(),
            action: "wipe_db".into(),
            request_id: "r1".into(),
            issued_at: 1000,
            expires_at: 1060,
            audit_hash: "sha256:abc".into(),
        }
    }

    #[test]
    fn valid_token_verifies_and_every_field_is_covered() {
        let k = ServerKey::ephemeral();
        let a = auth();
        let sig = k.sign_hex(a.message().as_bytes());
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", Some("bot"), 1010, 0), Ok(()));
        // Toute modification d'un champ casse la signature.
        for forged in [
            Authorization { agent: "eve".into(), ..auth() },
            Authorization { request_id: "r2".into(), ..auth() },
            Authorization { expires_at: 9999, ..auth() },
            Authorization { audit_hash: "sha256:zzz".into(), ..auth() },
        ] {
            assert_eq!(
                verify_authorization(&k.public_hex(), &forged, &sig, &forged.action, None, 1010, 0),
                Err(AuthzError::BadSignature)
            );
        }
    }

    #[test]
    fn wrong_key_action_agent_or_time_is_refused() {
        let k = ServerKey::ephemeral();
        let a = auth();
        let sig = k.sign_hex(a.message().as_bytes());
        let other = ServerKey::ephemeral().public_hex();
        assert_eq!(verify_authorization(&other, &a, &sig, "wipe_db", None, 1010, 0), Err(AuthzError::BadSignature));
        assert_eq!(verify_authorization("zz", &a, &sig, "wipe_db", None, 1010, 0), Err(AuthzError::BadKey));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "drop_table", None, 1010, 0), Err(AuthzError::WrongAction));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", Some("eve"), 1010, 0), Err(AuthzError::WrongAgent));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", None, 1061, 0), Err(AuthzError::Expired));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", None, 990, 5), Err(AuthzError::NotYetValid));
    }

    #[test]
    fn key_file_is_reused_and_public_key_published() {
        let dir = std::env::temp_dir().join(format!("cstl_srvkey_{}_{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("k");
        let a = ServerKey::load_or_create(&path).unwrap();
        let b = ServerKey::load_or_create(&path).unwrap();
        assert_eq!(a.public_hex(), b.public_hex());
        assert_eq!(std::fs::read_to_string(public_key_path(&path)).unwrap().trim(), a.public_hex());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
