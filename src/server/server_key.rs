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
const MARKER_VERSION: &str = "cstl-audit-marker-v2";

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

    /// Marqueur de la chaine d'audit, stocke a cote de la cle
    /// (`<cle>.audit_since`) et SIGNE (2026-11-20) :
    /// - `since` : premier `seq` qui DOIT etre signe. Les maillons plus
    ///   anciens datent d'avant la signature et restent verifies par leurs
    ///   liens et leur contenu. Sans ce seuil signe, un attaquant qui
    ///   reecrit la base pourrait retirer toutes les signatures et faire
    ///   passer tout l'historique pour "ancien" ;
    /// - `head` : dernier maillon ecrit (seq, hash), mis a jour a chaque
    ///   ajout. Une chaine qui s'arrete AVANT ce maillon a perdu ses derniers
    ///   maillons : ni les liens ni les signatures ne le montrent, puisque
    ///   tout ce qui reste est intact.
    ///
    /// Lecture : `Ok(None)` si absent, `Err` si present mais falsifie ou
    /// illisible. Le format sans `head` (premiere version du marqueur) est
    /// encore accepte : il est complete au demarrage suivant.
    pub fn read_audit_marker(&self) -> Result<Option<AuditMarker>, String> {
        let Some(key_path) = &self.path else { return Ok(None) };
        let path = audit_since_path(key_path);
        if !path.exists() {
            return Ok(None);
        }
        let falsified = || format!(
            "marqueur '{}' falsifie ou illisible : impossible de verifier l'etendue de la chaine d'audit signee",
            path.display()
        );
        let text = std::fs::read_to_string(&path).map_err(|e| format!("marqueur '{}' illisible: {e}", path.display()))?;
        let field = |name: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{name}=")).map(|v| v.trim().to_string()));
        let since = field("since").and_then(|v| v.parse::<u64>().ok()).ok_or_else(falsified)?;
        let sig = field("signature").ok_or_else(falsified)?;
        let head = match (field("head_seq"), field("head_hash")) {
            (Some(seq), Some(hash)) => Some((seq.parse::<u64>().map_err(|_| falsified())?, hash)),
            (None, None) => None,
            _ => return Err(falsified()),
        };
        let marker = AuditMarker { since, head };
        // Sans dernier maillon : format actuel (chaine vide) OU premiere
        // version du marqueur, qui ne connaissait que le seuil.
        let valid = self.verify_hex(marker.message().as_bytes(), &sig)
            || (marker.head.is_none() && self.verify_hex(format!("{SINCE_VERSION}\nsince={since}").as_bytes(), &sig));
        if valid { Ok(Some(marker)) } else { Err(falsified()) }
    }

    /// Ecrit le marqueur (fichier temporaire puis renommage : jamais de
    /// marqueur a moitie ecrit). Sans effet pour une cle ephemere.
    pub fn write_audit_marker(&self, marker: &AuditMarker) -> Result<(), String> {
        let Some(key_path) = &self.path else { return Ok(()) };
        let path = audit_since_path(key_path);
        let mut body = format!("since={}\n", marker.since);
        if let Some((seq, hash)) = &marker.head {
            body.push_str(&format!("head_seq={seq}\nhead_hash={hash}\n"));
        }
        body.push_str(&format!("signature={}\n", self.sign_hex(marker.message().as_bytes())));
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, body).map_err(|e| format!("ecriture de '{}' impossible: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("ecriture de '{}' impossible: {e}", path.display()))
    }

    /// Au demarrage : lit le marqueur, ou le cree. `chain_len` et `last` :
    /// la chaine telle que chargee ; `has_signed_entries` : la base contient
    /// deja au moins un maillon signe. Renvoie le marqueur a utiliser pour la
    /// verification d'integrite (son `head` est celui d'AVANT ce demarrage).
    ///
    /// Marqueur absent alors que la base contient des maillons signes : il a
    /// ete supprime (pour effacer le seuil ou le dernier maillon) -> erreur.
    pub fn init_audit_marker(&self, chain_len: u64, last: Option<(u64, String)>, has_signed_entries: bool) -> Result<AuditMarker, String> {
        if self.path.is_none() {
            return Ok(AuditMarker { since: chain_len, head: None });
        }
        match self.read_audit_marker()? {
            // Base VIDE alors que le marqueur connait des maillons : la base a
            // ete supprimee ou reinitialisee (AGENTS.md la dit supprimable).
            // On repart d'un marqueur neuf, avec une alerte. Ce n'est pas une
            // falsification silencieuse : l'historique entier a disparu, ce
            // qui se voit, et rien ne peut y etre remis sans la cle (tout
            // maillon a partir de seq=0 doit etre signe).
            Some(marker) if chain_len == 0 && marker.head.is_some() => {
                eprintln!(
                    "[CSTL-Native Server] ⚠️  historique d'audit VIDE alors que seq={} avait ete ecrit : base supprimee ou reinitialisee. Nouveau depart signe depuis seq=0.",
                    marker.head.as_ref().map(|h| h.0).unwrap_or(0)
                );
                let fresh = AuditMarker { since: 0, head: None };
                self.write_audit_marker(&fresh)?;
                Ok(fresh)
            }
            Some(marker) => {
                if marker.head.is_none() {
                    // Premiere version du marqueur (sans dernier maillon) :
                    // complete avec la chaine actuelle, une seule fois.
                    self.write_audit_marker(&AuditMarker { since: marker.since, head: last })?;
                }
                Ok(marker)
            }
            None if has_signed_entries => Err(
                "marqueur de la chaine d'audit absent alors que la base contient des maillons signes -- supprime ? demarrage refuse".to_string(),
            ),
            None => {
                let marker = AuditMarker { since: chain_len, head: last };
                self.write_audit_marker(&marker)?;
                Ok(AuditMarker { since: chain_len, head: None })
            }
        }
    }
}

/// Contenu signe du marqueur de chaine (`ServerKey::read_audit_marker`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditMarker {
    pub since: u64,
    /// Dernier maillon ecrit : (seq, hash).
    pub head: Option<(u64, String)>,
}

impl AuditMarker {
    fn message(&self) -> String {
        match &self.head {
            Some((seq, hash)) => format!("{MARKER_VERSION}\nsince={}\nhead_seq={seq}\nhead_hash={hash}", self.since),
            None => format!("{MARKER_VERSION}\nsince={}\nhead=none", self.since),
        }
    }
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
