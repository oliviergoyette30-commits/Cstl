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
    /// Anciennes cles du serveur (2026-11-21, `rotate`), la plus recente
    /// d'abord. Chacune ne vaut que pour les maillons ecrits AVANT sa
    /// retraite (`until_seq`) : une ancienne cle volee ne permet pas de
    /// signer un faux maillon recent.
    past: Vec<PastKey>,
}

#[derive(Clone)]
pub struct PastKey {
    pub public_hex: String,
    key: VerifyingKey,
    /// Premier `seq` signe par la cle SUIVANTE.
    pub until_seq: u64,
}

const ROTATION_VERSION: &str = "cstl-key-rotation-v1";

fn rotation_message(old: &str, new: &str, at_seq: u64) -> String {
    format!("{ROTATION_VERSION}\nold={old}\nnew={new}\nat_seq={at_seq}")
}

pub fn rotations_path(key_path: &Path) -> PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".rotations");
    PathBuf::from(p)
}

fn verifying(hex_key: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = hex::decode(hex_key).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

fn verify_with(key: &VerifyingKey, message: &[u8], signature_hex: &str) -> bool {
    let Some(bytes) = hex::decode(signature_hex).ok().and_then(|b| <[u8; 64]>::try_from(b).ok()) else {
        return false;
    };
    key.verify(message, &Signature::from_bytes(&bytes)).is_ok()
}

/// Reconstruit la lignee des anciennes cles a partir de la cle actuelle.
/// Chaque passage de relais est signe par l'ancienne ET par la nouvelle
/// cle : un fichier modifie, ou un relais insere sans les deux cles, fait
/// echouer le chargement (et donc le demarrage).
fn load_past_keys(key_path: &Path, current_hex: &str) -> Result<Vec<PastKey>, String> {
    let path = rotations_path(key_path);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("'{}' illisible: {e}", path.display()))?;
    let records: Vec<serde_json::Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("'{}' : ligne illisible ({e})", path.display())))
        .collect::<Result<_, _>>()?;
    let field = |r: &serde_json::Value, k: &str| r.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let mut past = Vec::new();
    let mut cur = current_hex.to_string();
    let mut last_at = u64::MAX;
    while let Some(r) = records.iter().find(|r| field(r, "new").as_deref() == Some(cur.as_str())) {
        let bad = || format!("'{}' : passage de cle falsifie (vers {}) -- demarrage refuse", path.display(), &cur[..16.min(cur.len())]);
        let old = field(r, "old").ok_or_else(bad)?;
        let at_seq = r.get("at_seq").and_then(|v| v.as_u64()).ok_or_else(bad)?;
        let (Some(old_key), Some(new_key)) = (verifying(&old), verifying(&cur)) else { return Err(bad()) };
        let msg = rotation_message(&old, &cur, at_seq);
        let signed_by_both = verify_with(&old_key, msg.as_bytes(), &field(r, "sig_old").unwrap_or_default())
            && verify_with(&new_key, msg.as_bytes(), &field(r, "sig_new").unwrap_or_default());
        if !signed_by_both || at_seq > last_at || past.iter().any(|p: &PastKey| p.public_hex == old) {
            return Err(bad());
        }
        last_at = at_seq;
        past.push(PastKey { public_hex: old.clone(), key: old_key, until_seq: at_seq });
        cur = old;
    }
    Ok(past)
}

impl ServerKey {
    pub fn ephemeral() -> Self {
        ServerKey { signing: SigningKey::generate(&mut rand::rngs::OsRng), path: None, past: Vec::new() }
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
        let past = load_past_keys(path, &public)?;
        Ok(ServerKey { signing, path: Some(path.to_path_buf()), past })
    }

    pub fn past_keys(&self) -> &[PastKey] {
        &self.past
    }

    /// Verifie la signature d'un maillon de rang `seq` : cle actuelle, ou
    /// ancienne cle encore en service a ce rang.
    pub fn verify_audit(&self, seq: u64, message: &[u8], signature_hex: &str) -> bool {
        self.verify_hex(message, signature_hex)
            || self.past.iter().any(|p| seq < p.until_seq && verify_with(&p.key, message, signature_hex))
    }

    /// Comme `verify_audit`, pour un maillon qui annonce la cle qui l'a signe.
    pub fn verify_audit_by(&self, public_hex: &str, seq: u64, message: &[u8], signature_hex: &str) -> bool {
        if public_hex.eq_ignore_ascii_case(&self.public_hex()) {
            return self.verify_hex(message, signature_hex);
        }
        self.past.iter().any(|p| p.public_hex.eq_ignore_ascii_case(public_hex) && seq < p.until_seq && verify_with(&p.key, message, signature_hex))
    }

    /// Remplace la cle du serveur (2026-11-21). `at_seq` : longueur actuelle
    /// de la chaine -- les maillons a partir de ce rang seront signes par la
    /// nouvelle cle. Le passage de relais est signe par les DEUX cles et
    /// ajoute a `<cle>.rotations` ; l'ancienne cle privee est ecrasee, sa cle
    /// publique reste valable pour les maillons anterieurs. Renvoie la
    /// nouvelle cle. Le marqueur de chaine doit ensuite etre re-signe.
    pub fn rotate(&self, at_seq: u64) -> Result<ServerKey, String> {
        let Some(path) = &self.path else { return Err("cle ephemere : rien a remplacer".into()) };
        let new = SigningKey::generate(&mut rand::rngs::OsRng);
        let (old_hex, new_hex) = (self.public_hex(), hex::encode(new.verifying_key().to_bytes()));
        let msg = rotation_message(&old_hex, &new_hex, at_seq);
        let record = serde_json::json!({
            "old": old_hex, "new": new_hex, "at_seq": at_seq,
            "sig_old": self.sign_hex(msg.as_bytes()),
            "sig_new": hex::encode(new.sign(msg.as_bytes()).to_bytes()),
        });
        let rot = rotations_path(path);
        let mut text = if rot.exists() {
            std::fs::read_to_string(&rot).map_err(|e| format!("'{}' illisible: {e}", rot.display()))?
        } else {
            String::new()
        };
        text.push_str(&format!("{record}\n"));
        atomic_write(&rot, &text, false)?;
        atomic_write(path, &hex::encode(new.to_bytes()), true)?;
        ServerKey::load_or_create(path)
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

/// Supprime les fichiers de cle crees a cote d'une base (`for_data_path`) :
/// pour les tests et exemples qui jettent leur base temporaire, afin de ne
/// pas laisser de cles privees derriere eux.
pub fn remove_key_files(data_path: &str) {
    for suffix in [".server_key", ".server_key.pub", ".server_key.audit_since", ".server_key.audit_since.tmp",
                   ".server_key.rotations", ".server_key.rotations.tmp", ".server_key.tmp"] {
        let _ = std::fs::remove_file(format!("{data_path}{suffix}"));
    }
}

pub fn public_key_path(key_path: &Path) -> PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".pub");
    PathBuf::from(p)
}

/// Fichier temporaire puis renommage ; `private` : droits 0600.
fn atomic_write(path: &Path, content: &str, private: bool) -> Result<(), String> {
    use std::io::Write;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut f = opts.open(&tmp).map_err(|e| format!("ecriture de '{}' impossible: {e}", tmp.display()))?;
    f.write_all(content.as_bytes()).map_err(|e| format!("ecriture de '{}' impossible: {e}", tmp.display()))?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| format!("ecriture de '{}' impossible: {e}", path.display()))
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
    /// Montant autorise (2026-11-21), si la demande en portait un : le
    /// systeme vise doit executer CE montant, pas un autre.
    pub amount: Option<String>,
}

impl Authorization {
    /// Message canonique signe. Une ligne par champ, ordre fixe : aucun
    /// champ ne peut contenir de saut de ligne (valeurs du wire CSTL).
    pub fn message(&self) -> String {
        format!(
            "{AUTHZ_VERSION}\nagent={}\naction={}\nrequest_id={}\nissued_at={}\nexpires_at={}\naudit_hash={}",
            self.agent, self.action, self.request_id, self.issued_at, self.expires_at, self.audit_hash
        ) + &self.amount.as_deref().map(|a| format!("\namount={a}")).unwrap_or_default()
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
    /// Le montant execute n'est pas celui du jeton (2026-11-25).
    WrongAmount,
}

/// Verification cote systeme vise. `now` : secondes unix ; `skew` : tolerance
/// d'horloge. Ne verifie PAS le rejeu : au systeme vise de refuser un
/// `request_id` deja utilise (voir `cstl_guard.py`).
///
/// `executed_amount` (2026-11-25) : le montant que le systeme vise va
/// REELLEMENT executer. Meme regle que `CstlGuard.check` : si le jeton ou
/// l'execution porte un montant, les deux doivent exister et etre egaux
/// (comparaison numerique). Avant, cette fonction ne regardait pas le
/// montant : un systeme en Rust aurait execute 8000 avec un jeton de 80
/// (trouve en ecrivant la specification de conformite v1).
// API publique stable (spec de conformite v1, appelee telle quelle par les
// systemes cibles en Rust) : chaque argument est un champ verifie du jeton.
#[allow(clippy::too_many_arguments)]
pub fn verify_authorization(
    server_public_hex: &str,
    auth: &Authorization,
    signature_hex: &str,
    expected_action: &str,
    expected_agent: Option<&str>,
    executed_amount: Option<&str>,
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
    if executed_amount.is_some() || auth.amount.is_some() {
        let num = |x: Option<&str>| x.and_then(|v| v.trim().parse::<f64>().ok()).filter(|f| f.is_finite());
        match (num(executed_amount), num(auth.amount.as_deref())) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(AuthzError::WrongAmount),
        }
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
            amount: None,
        }
    }

    #[test]
    fn valid_token_verifies_and_every_field_is_covered() {
        let k = ServerKey::ephemeral();
        let a = auth();
        let sig = k.sign_hex(a.message().as_bytes());
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", Some("bot"), None, 1010, 0), Ok(()));
        // Toute modification d'un champ casse la signature.
        for forged in [
            Authorization { agent: "eve".into(), ..auth() },
            Authorization { request_id: "r2".into(), ..auth() },
            Authorization { expires_at: 9999, ..auth() },
            Authorization { audit_hash: "sha256:zzz".into(), ..auth() },
        ] {
            assert_eq!(
                verify_authorization(&k.public_hex(), &forged, &sig, &forged.action, None, None, 1010, 0),
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
        assert_eq!(verify_authorization(&other, &a, &sig, "wipe_db", None, None, 1010, 0), Err(AuthzError::BadSignature));
        assert_eq!(verify_authorization("zz", &a, &sig, "wipe_db", None, None, 1010, 0), Err(AuthzError::BadKey));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "drop_table", None, None, 1010, 0), Err(AuthzError::WrongAction));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", Some("eve"), None, 1010, 0), Err(AuthzError::WrongAgent));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", None, None, 1061, 0), Err(AuthzError::Expired));
        assert_eq!(verify_authorization(&k.public_hex(), &a, &sig, "wipe_db", None, None, 990, 5), Err(AuthzError::NotYetValid));
    }

    #[test]
    fn executed_amount_must_match_the_token() {
        let k = ServerKey::ephemeral();
        let a = Authorization { action: "refund_order".into(), amount: Some("80".into()), ..auth() };
        let sig = k.sign_hex(a.message().as_bytes());
        let v = |amt| verify_authorization(&k.public_hex(), &a, &sig, "refund_order", None, amt, 1010, 0);
        assert_eq!(v(Some("80")), Ok(()));
        assert_eq!(v(Some("80.0")), Ok(()));
        assert_eq!(v(Some("8000")), Err(AuthzError::WrongAmount));
        assert_eq!(v(None), Err(AuthzError::WrongAmount));
        // Jeton sans montant, execution avec un montant : refuse aussi.
        let b = auth();
        let sig_b = k.sign_hex(b.message().as_bytes());
        assert_eq!(verify_authorization(&k.public_hex(), &b, &sig_b, "wipe_db", None, Some("5"), 1010, 0),
                   Err(AuthzError::WrongAmount));
    }

    #[test]
    fn rotation_keeps_old_links_valid_only_before_the_switch() {
        let dir = std::env::temp_dir().join(format!("cstl_rot_{}_{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("k");
        let k1 = ServerKey::load_or_create(&path).unwrap();
        let m = b"maillon";
        let old_sig = k1.sign_hex(m);
        let k2 = k1.rotate(10).unwrap();
        assert_ne!(k1.public_hex(), k2.public_hex());
        // Rechargee depuis le disque : la lignee est reconstruite.
        let k2 = ServerKey::load_or_create(&path).unwrap();
        assert_eq!(k2.past_keys().len(), 1);
        assert!(k2.verify_audit(9, m, &old_sig), "ancien maillon, ancienne cle : valide");
        assert!(!k2.verify_audit(10, m, &old_sig), "maillon recent signe avec l'ancienne cle : refuse");
        assert!(k2.verify_audit(10, m, &k2.sign_hex(m)));
        // Deuxieme rotation : les deux anciennes cles restent bornees.
        let k3 = k2.rotate(20).unwrap();
        assert_eq!(k3.past_keys().iter().map(|p| p.until_seq).collect::<Vec<_>>(), [20, 10]);
        assert!(k3.verify_audit(5, m, &old_sig) && !k3.verify_audit(15, m, &old_sig));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // Lignee falsifiee : chargement refuse.
        let rot = rotations_path(&path);
        let tampered = std::fs::read_to_string(&rot).unwrap().replace("\"at_seq\":10", "\"at_seq\":99");
        std::fs::write(&rot, tampered).unwrap();
        assert!(ServerKey::load_or_create(&path).err().unwrap().contains("falsifie"));
        std::fs::remove_dir_all(&dir).ok();
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
