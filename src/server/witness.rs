//! src/server/witness.rs -- TEMOIN externe de la chaine d'audit : detecter un
//! retour en arriere complet (2026-11-20).
//!
//! Probleme : la signature des maillons et le marqueur signe du dernier
//! maillon (server_key.rs) vivent sur la MEME machine que la base. Quelqu'un
//! qui remet ensemble une ancienne copie de la base ET du marqueur obtient un
//! etat ancien, mais parfaitement coherent et signe : rien sur la machine ne
//! permet de savoir qu'il manque la suite.
//!
//! Solution : un temoin, sur une AUTRE machine (`sdk/python/cstl_witness.py`,
//! `CSTL_WITNESS_URL`), garde le dernier maillon signe qu'il a vu. Le serveur
//! le lui envoie apres chaque ajout. Au demarrage, il lui demande ce dernier
//! maillon : si la chaine locale s'arrete avant, ou a un autre maillon a ce
//! rang, c'est un retour en arriere (ou une fourche) -> demarrage refuse.
//!
//! Le temoin ne peut pas faire accepter un faux etat : ce qu'il renvoie doit
//! porter une signature valide de la cle du serveur. Il peut seulement
//! bloquer le demarrage, ce qui est visible. Limite : l'envoi est asynchrone,
//! les tout derniers maillons ecrits juste avant un arret brutal peuvent ne
//! pas avoir ete vus par le temoin.

use serde::{Deserialize, Serialize};

use super::server_key::{audit_message, ServerKey};
use crate::server::audit::AuditEntry;

/// Un maillon signe, tel que transmis au temoin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub server_key: String,
    pub seq: u64,
    pub hash: String,
    pub parent_hash: String,
    pub sender: String,
    pub receiver: String,
    pub purpose: String,
    pub signature: String,
}

impl Checkpoint {
    pub fn new(key: &ServerKey, entry: &AuditEntry, signature: String) -> Self {
        Checkpoint {
            server_key: key.public_hex(),
            seq: entry.seq,
            hash: entry.hash.clone(),
            parent_hash: entry.parent_hash.clone(),
            sender: entry.sender.clone(),
            receiver: entry.receiver.clone(),
            purpose: entry.purpose.clone(),
            signature,
        }
    }

    fn message(&self) -> String {
        audit_message(self.seq, &self.hash, &self.parent_hash, &self.sender, &self.receiver, &self.purpose)
    }
}

/// Compare la chaine locale au dernier maillon vu par le temoin.
pub fn check_against_witness(key: &ServerKey, local: &[AuditEntry], witnessed: &Checkpoint) -> Result<(), String> {
    // Cle actuelle, ou ancienne cle (rotation) encore en service a ce rang.
    if !key.verify_audit_by(&witnessed.server_key, witnessed.seq, witnessed.message().as_bytes(), &witnessed.signature) {
        return Err(format!(
            "le temoin renvoie un maillon (seq={}) qui n'est pas signe par la cle de ce serveur -- temoin mal configure ou compromis",
            witnessed.seq
        ));
    }
    match local.iter().find(|e| e.seq == witnessed.seq) {
        Some(e) if e.hash == witnessed.hash => Ok(()),
        Some(e) => Err(format!(
            "fourche : au rang seq={}, la chaine locale a le maillon {} mais le temoin a vu {} -- historique remplace",
            witnessed.seq,
            e.hash.chars().take(19).collect::<String>(),
            witnessed.hash.chars().take(19).collect::<String>()
        )),
        None => Err(format!(
            "retour en arriere : la chaine locale s'arrete a seq={} alors que le temoin a vu seq={} -- ancienne copie de la base restauree ?",
            local.last().map(|e| e.seq.to_string()).unwrap_or_else(|| "(vide)".into()),
            witnessed.seq
        )),
    }
}

/// Client HTTP du temoin (`CSTL_WITNESS_URL`).
#[derive(Clone)]
pub struct WitnessClient {
    base: String,
    http: reqwest::Client,
}

impl WitnessClient {
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("CSTL_WITNESS_URL").ok()?;
        let base = url.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            return None;
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok()?;
        Some(WitnessClient { base, http })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Dernier maillon vu par le temoin pour cette cle (`None` : aucun).
    pub async fn fetch(&self, server_key_hex: &str) -> Result<Option<Checkpoint>, String> {
        let resp = self
            .http
            .get(format!("{}/checkpoint/{}", self.base, server_key_hex))
            .send()
            .await
            .map_err(|e| format!("temoin injoignable ({}): {e}", self.base))?;
        match resp.status().as_u16() {
            404 => Ok(None),
            200 => resp.json::<Checkpoint>().await.map(Some).map_err(|e| format!("reponse du temoin illisible: {e}")),
            code => Err(format!("temoin {} : HTTP {code}", self.base)),
        }
    }

    pub async fn push(&self, cp: &Checkpoint) -> Result<(), String> {
        let resp = self
            .http
            .post(format!("{}/checkpoint", self.base))
            .json(cp)
            .send()
            .await
            .map_err(|e| format!("temoin injoignable ({}): {e}", self.base))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let code = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("temoin {} a refuse seq={} : HTTP {code} {body}", self.base, cp.seq))
        }
    }

    /// Envoie au temoin le dernier maillon publie dans `rx`, a chaque
    /// changement. Les maillons intermediaires peuvent etre sautes : seul le
    /// plus recent compte.
    pub fn spawn_pusher(self, mut rx: tokio::sync::watch::Receiver<Option<Checkpoint>>) {
        tokio::spawn(async move {
            loop {
                let latest = rx.borrow_and_update().clone();
                if let Some(cp) = latest {
                    if let Err(e) = self.push(&cp).await {
                        eprintln!("[Temoin] ⚠️  {e}");
                    }
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(key: &ServerKey, n: u64) -> (Vec<AuditEntry>, Vec<Checkpoint>) {
        let mut entries = Vec::new();
        let mut cps = Vec::new();
        let mut parent = "root".to_string();
        for seq in 0..n {
            let e = AuditEntry {
                hash: format!("sha256:{seq:064}"),
                parent_hash: parent.clone(),
                sender: "alice".into(),
                receiver: "bob".into(),
                purpose: "inform".into(),
                seq,
            };
            parent = e.hash.clone();
            let sig = key.sign_hex(audit_message(e.seq, &e.hash, &e.parent_hash, &e.sender, &e.receiver, &e.purpose).as_bytes());
            cps.push(Checkpoint::new(key, &e, sig));
            entries.push(e);
        }
        (entries, cps)
    }

    #[test]
    fn up_to_date_or_ahead_chain_passes() {
        let key = ServerKey::ephemeral();
        let (entries, cps) = chain(&key, 5);
        assert_eq!(check_against_witness(&key, &entries, &cps[4]), Ok(()));
        // Le temoin peut etre en retard (envoi asynchrone) : pas une alerte.
        assert_eq!(check_against_witness(&key, &entries, &cps[2]), Ok(()));
    }

    #[test]
    fn rollback_and_fork_are_refused() {
        let key = ServerKey::ephemeral();
        let (entries, cps) = chain(&key, 5);
        let err = check_against_witness(&key, &entries[..3], &cps[4]).unwrap_err();
        assert!(err.contains("retour en arriere") && err.contains("seq=2") && err.contains("seq=4"), "{err}");
        let mut forked = entries.clone();
        forked[4].hash = "sha256:autre".into();
        assert!(check_against_witness(&key, &forked, &cps[4]).unwrap_err().contains("fourche"));
    }

    #[test]
    fn a_lying_witness_cannot_be_trusted() {
        let key = ServerKey::ephemeral();
        let (entries, mut cps) = chain(&key, 3);
        // Un temoin qui invente un maillon plus loin (sans la cle) est demasque.
        cps[2].seq = 99;
        assert!(check_against_witness(&key, &entries, &cps[2]).unwrap_err().contains("pas signe"));
        // ... comme un maillon signe par une autre cle.
        let other = ServerKey::ephemeral();
        let (_, other_cps) = chain(&other, 3);
        assert!(check_against_witness(&key, &entries, &other_cps[2]).unwrap_err().contains("pas signe"));
    }
}
