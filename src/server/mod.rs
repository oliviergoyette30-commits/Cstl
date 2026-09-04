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
    /// Seede depuis `audit_store` au demarrage (voir `with_data_path`) --
    /// avant le 2026-09-04, toujours vide a la construction (HashChain::new()),
    /// meme quand `audit_store`/`adn_store` avaient deja de l'historique sur
    /// disque. Reste vrai que chaque `append()` en cours de session est
    /// PUREMENT en memoire tant que `handler.rs` n'a pas aussi appele
    /// `audit_store.save(&entry)` juste apres -- c'est ce deuxieme appel qui
    /// rend la seed du PROCHAIN demarrage possible.
    pub chain: Arc<Mutex<audit::HashChain>>,
    /// Persistance de `chain` (Couche 5/8, cable live le 2026-09-04 -- voir
    /// le commentaire de tete de `audit_store.rs` pour la trouvaille: ce
    /// module existait, teste, mais n'etait appele nulle part avant cette
    /// passe). Deuxieme `Connection` SQLite vers le MEME fichier que
    /// `adn_store` (pas encore une fusion en un seul schema/une seule
    /// connexion -- portee assumee, voir le commentaire de `audit_store.rs`).
    pub audit_store: Arc<Mutex<audit_store::AuditStore>>,
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
    /// chaque champ apres coup. `adn_store` ET `audit_store` pointent sur le
    /// MEME `data_path` -- deux `Connection` SQLite distinctes vers un seul
    /// fichier, pas encore une fusion en un seul schema (voir le commentaire
    /// de tete de `audit_store.rs`).
    pub fn with_data_path(port: u16, data_path: &str) -> Self {
        let audit_store = audit_store::AuditStore::open(data_path)
            .expect("failed to open audit_store at data_path");
        // Seed la chaine en memoire depuis ce qui est deja persiste --
        // AVANT ce fix (2026-09-04), ce chargement n'existait pas du tout:
        // `chain` demarrait toujours vide, meme quand cette meme base SQLite
        // contenait deja des payloads avec leur propre lignee de parent_hash
        // (adn_store.rs). Un redemarrage rompait donc silencieusement la
        // continuite de la chaine de hachage.
        let chain = audit_store.load_chain().expect("failed to load persisted audit chain");

        CstlNativeServer {
            port,
            agent_registry: Arc::new(Mutex::new(AgentRegistry::new())),
            chain: Arc::new(Mutex::new(chain)),
            audit_store: Arc::new(Mutex::new(audit_store)),
            kb_verifier: Arc::new(KbVerifier::new()),
            adn_store: Arc::new(Mutex::new(AdnStore::open(data_path).expect("failed to open cstl_adn.db"))),
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
        
        listener::accept_connections(listener, self.agent_registry.clone(), self.chain.clone(), self.audit_store.clone(), self.kb_verifier.clone(), self.adn_store.clone(), self.restricted_council.clone(), self.telegram.clone(), self.obsidian.clone(), self.governance.clone()).await?;
        
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

pub mod audit_store;
