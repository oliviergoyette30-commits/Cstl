/// CSTL-Native Server
/// Agent-to-agent communication natively in CSTL
/// 
/// Architecture:
/// 1. TCP Listener (incoming CSTL payloads)
/// 2. Parser (validates CSTL format)
/// 3. Validator (semantic checking)
/// 4. Router (finds destination agent)
/// 5. Audit Trail (immutable SHA-256 record)

pub mod listener;
pub mod handler;

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use crate::restricted_council::RestrictedCouncil;
use crate::telegram_council::TelegramNotifier;
use crate::obsidian_escalation::ObsidianEscalation;
use crate::governance::GovernanceTracker;

pub struct CstlNativeServer {
    pub port: u16,
    /// Mutable derriere un verrou depuis l'ajout de purpose=agent_register
    /// (2026-09-04) -- avant ca, le registre etait fige a la compilation
    /// (alice/bob en dur dans main.rs), aucun agent ne pouvait s'inscrire
    /// au runtime.
    pub agent_registry: Arc<Mutex<AgentRegistry>>,
    /// Seede depuis `adn_store` au demarrage (voir `with_data_path`) --
    /// avant le 2026-09-04, toujours vide a la construction (HashChain::new()),
    /// meme quand la base SQLite avait deja de l'historique sur disque.
    /// Reste vrai que chaque `append()` en cours de session est PUREMENT en
    /// memoire tant que `handler.rs` n'a pas aussi appele
    /// `adn_store.save_audit_entry(&entry)` juste apres -- c'est ce deuxieme
    /// appel qui rend la seed du PROCHAIN demarrage possible.
    pub chain: Arc<Mutex<audit::HashChain>>,
    /// Fusion 2026-09-04 (item #1 de la liste des choses a faire, apres
    /// fix19): la chaine d'audit (table `audit_trail`, ex-module
    /// `server/audit_store.rs`) et l'historique ADN (`adn_store`/
    /// `adn_relations`) vivaient sur le MEME fichier SQLite via DEUX
    /// `Connection` distinctes, chacune derriere son propre `Mutex` en
    /// memoire -- un vrai risque de coordination (deux verrous logiques pour
    /// un seul fichier physique), pas seulement de la dette cosmetique.
    /// `AdnStore` porte maintenant les deux schemas (une seule `Connection`,
    /// un seul `Arc<Mutex<..>>`) -- voir `adn_store.rs::save_audit_entry`/
    /// `load_chain`/`audit_count`.
    pub kb_verifier: Arc<KbVerifier>,
    pub adn_store: Arc<Mutex<AdnStore>>,
    pub restricted_council: Arc<RestrictedCouncil>,
    pub telegram: Option<Arc<TelegramNotifier>>,
    pub obsidian: Option<Arc<ObsidianEscalation>>,
    /// Couche 2 (gouvernance/resilience) -- circuit breaker + drift
    /// d'operateur, observation seule (voir src/governance.rs). Etat en
    /// memoire uniquement, perdu au redemarrage -- limite v1 assumee.
    pub governance: Arc<Mutex<GovernanceTracker>>,
}

impl CstlNativeServer {
    pub fn new(port: u16) -> Self {
        Self::with_data_path(port, "cstl_adn.db")
    }

    /// Comme `new`, mais avec le chemin de la base SQLite explicite --
    /// utilisee par `new` (avec "cstl_adn.db", chemin qui etait fige en dur
    /// avant ce refactor) et par les smoke-tests/tests qui veulent une base
    /// isolee (":memory:") sans passer par une reconstruction manuelle de
    /// chaque champ apres coup. Une seule `Connection` SQLite vers
    /// `data_path` depuis la fusion du 2026-09-04 (voir le commentaire de
    /// `adn_store` ci-dessus).
    pub fn with_data_path(port: u16, data_path: &str) -> Self {
        let adn_store = AdnStore::open(data_path).expect("failed to open cstl_adn.db");
        // Seed la chaine en memoire depuis ce qui est deja persiste --
        // AVANT le 2026-09-04, ce chargement n'existait pas du tout:
        // `chain` demarrait toujours vide, meme quand cette meme base SQLite
        // contenait deja des payloads avec leur propre lignee de parent_hash.
        // Un redemarrage rompait donc silencieusement la continuite de la
        // chaine de hachage.
        let chain = adn_store.load_chain().expect("failed to load persisted audit chain");

        CstlNativeServer {
            port,
            agent_registry: Arc::new(Mutex::new(AgentRegistry::new())),
            chain: Arc::new(Mutex::new(chain)),
            kb_verifier: Arc::new(KbVerifier::new()),
            adn_store: Arc::new(Mutex::new(adn_store)),
            // Portee reduite v1, decision explicite de l'utilisateur: un seul membre
            // autorise pour bootstrap le systeme, pas le quorum 2/3 multi-personnes
            // decrit dans le README.
            // Config production (2026-09-04): CSTL_COUNCIL_MEMBERS (noms
            // separes par des virgules) permet un vrai conseil multi-membres;
            // absent -> single_member("Olivier"), comportement identique a
            // avant ce changement. Voir restricted_council.rs::from_env()
            // pour le detail, et handler.rs (bloc council_decision) pour la
            // verification de signature qui rend ce quorum reellement
            // infalsifiable (pas seulement arithmetiquement correct).
            restricted_council: Arc::new(RestrictedCouncil::from_env()),
            // None si TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID absents de l'environnement -
            // degradation propre, le serveur marche pareil sans notification.
            telegram: TelegramNotifier::from_env().map(Arc::new),
            // None si OBSIDIAN_VAULT_PATH absent de l'environnement - degradation
            // propre, le serveur marche pareil sans escalade Obsidian.
            obsidian: ObsidianEscalation::from_env().map(Arc::new),
            governance: Arc::new(Mutex::new(GovernanceTracker::with_defaults())),
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[CSTL-Native Server] Starting on port {}", self.port);

        if let Some(telegram) = &self.telegram {
            eprintln!("[CSTL-Native Server] Telegram poller actif");
            let telegram = telegram.clone();
            let adn_store = self.adn_store.clone();
            let restricted_council = self.restricted_council.clone();
            tokio::spawn(async move {
                crate::telegram_council::run_telegram_poller(telegram, adn_store, restricted_council).await;
            });
        } else {
            eprintln!("[CSTL-Native Server] Telegram desactive (TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID absents)");
        }
        
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = listener::create_listener(&addr).await?;
        
        eprintln!("[CSTL-Native Server] Listening on {}", addr);
        
        listener::accept_connections(listener, self.agent_registry.clone(), self.chain.clone(), self.kb_verifier.clone(), self.adn_store.clone(), self.restricted_council.clone(), self.telegram.clone(), self.obsidian.clone(), self.governance.clone()).await?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = CstlNativeServer::new(5000);
        assert_eq!(server.port, 5000);
    }
}

pub mod parser;

pub mod validator;

pub mod audit;
