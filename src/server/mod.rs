//! CSTL-Native Server
//! Agent-to-agent communication natively in CSTL
//!
//! Architecture:
//! 1. TCP Listener (incoming CSTL payloads)
//! 2. Parser (validates CSTL format)
//! 3. Validator (semantic checking)
//! 4. Router (finds destination agent)
//! 5. Audit Trail (immutable SHA-256 record)

pub mod listener;
pub mod handler;
pub mod quorum;
pub mod quorum_wire;
pub mod arbitrage;
pub mod castle;
pub mod castle_wire;
pub mod wai;
pub mod parser;
pub mod validator;
pub mod audit;
pub mod tls;
pub mod rest_api;
pub mod deontic_orchestration;
pub mod deontic_state_machine;
pub mod graphify_server;
pub mod evaluated_payload;
pub mod response_compression;

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use serde_json;

use crate::agent_discovery::AgentRegistry;
use crate::kb_verify::KbVerifier;
use crate::adn_store::AdnStore;
use crate::restricted_council::RestrictedCouncil;
use crate::telegram_council::TelegramNotifier;
use crate::obsidian_escalation::ObsidianEscalation;
use crate::governance::GovernanceTracker;
use crate::calibration::SigmaCalibrator;

/// Contexte serveur partagé — regroupe tous les sous-systèmes accessibles
/// par une connexion (registre, chaîne d'audit, ADN store, conseil, etc.)
/// en un seul Arc<ServerContext> plutôt que 9 paramètres séparés.
/// Allége la signature de handle_connection (Couche 7, 2026-09-04).
pub struct ServerContext {
    pub agent_registry: Arc<RwLock<AgentRegistry>>,
    pub chain: Arc<Mutex<audit::HashChain>>,
    pub kb_verifier: Arc<KbVerifier>,
    pub adn_store: Arc<Mutex<AdnStore>>,
    pub restricted_council: Arc<RestrictedCouncil>,
    pub telegram: Option<Arc<TelegramNotifier>>,
    pub obsidian: Option<Arc<ObsidianEscalation>>,
    pub governance: Arc<Mutex<GovernanceTracker>>,
    /// Couche 5 (calibration sigma, Double Livre v5.2) -- EWMA per-agent
    /// pour standardiser sigma entre les backends LLM (Claude/Gemini/Hermes).
    /// Alpha = 0.2 pour adaptation rapide (~50 message effective window).
    pub sigma_calibrator: Arc<Mutex<SigmaCalibrator>>,
    /// Couche 9 (deontic orchestration, 2026-10-01) -- moteur d'evenements +
    /// regles MUST/MUST_NOT/MAY (voir `deontic_orchestration.rs`). Trouve
    /// dead code par le meme grep que CASTLE/quorum/tls/graphify: module
    /// reel (15 tests d'integration passent, `tests/
    /// deontic_orchestration_integration_test.rs`) mais jamais construit ni
    /// appele nulle part avant ce commit, malgre le README qui affirmait
    /// "v5.1 COMPLETE". Cable ici en couche D'OBSERVATION/AUDIT sur le
    /// pipeline d'arbitrage deja reel (`arbitrage.rs`) et de gouvernance
    /// (`governance.rs`) -- PAS en couche de blocage: `emit_event` est
    /// appele APRES que l'action sous-jacente (enregistrement d'agent,
    /// ruling d'arbitrage) a deja ete commise, et `execute_must_rule`/
    /// `execute_must_not_rule` dans `deontic_orchestration.rs` ne font que
    /// logger (`eprintln!`) -- ils ne rejettent/bloquent rien reellement.
    /// Honnete: c'est un journal de regles deontiques appliquees apres
    /// coup, pas une garde preventive. Voir README pour le detail.
    pub deontic: Arc<deontic_orchestration::DeonticOrchestrator>,
    /// Corpus de collecte des reponses plain-text (2026-10-01, voir
    /// `adn_store::record_response_corpus_entry`) -- opt-in explicite via
    /// la variable d'environnement CSTL_COLLECT_RESPONSE_CORPUS, lue UNE
    /// fois au demarrage (voir `CstlNativeServer::start`), jamais active
    /// par defaut. Objectif: accumuler du vrai vocabulaire de reponse pour
    /// pouvoir un jour reentrainer text_dictionary/stable_dictionary --
    /// jusqu'ici bloque par l'absence de ce genre de donnees (voir
    /// README, section Response-side compression).
    pub collect_response_corpus: bool,
}

pub struct CstlNativeServer {
    pub port: u16,
    /// Mutable derriere un verrou depuis l'ajout de purpose=agent_register
    /// (2026-09-04) -- avant ca, le registre etait fige a la compilation
    /// (alice/bob en dur dans main.rs), aucun agent ne pouvait s'inscrire
    /// au runtime. Upgrade 2026-09-14: RwLock au lieu de Mutex pour
    /// concurrence lecture massale (STEP 2a verifies la signature sur CHAQUE
    /// message, jamais en écriture -- RwLock permet N lectures simultanees).
    pub agent_registry: Arc<RwLock<AgentRegistry>>,
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
    /// d'operateur, observation seule (voir src/governance.rs). Seede
    /// depuis `adn_store` au demarrage (voir `try_with_data_path`, meme
    /// patron que `chain` juste au-dessus) depuis le 2026-09-05 -- avant
    /// ca, toujours reconstruit vide (`with_defaults()`), meme quand la
    /// base SQLite avait deja de l'historique de breaker/drift sur disque.
    pub governance: Arc<Mutex<GovernanceTracker>>,
    /// Couche 5 (calibration sigma, Double Livre v5.2) -- EWMA per-agent
    /// pour standardiser sigma entre les backends LLM (Claude/Gemini/Hermes).
    /// Alpha = 0.2 pour adaptation rapide (~50 message effective window).
    pub sigma_calibrator: Arc<Mutex<SigmaCalibrator>>,
    /// Couche 9 -- voir le champ identique sur `ServerContext` ci-dessus
    /// pour le detail; `start()` clone cet `Arc` dans le `ServerContext`
    /// comme pour tous les autres sous-systemes partages.
    pub deontic: Arc<deontic_orchestration::DeonticOrchestrator>,
    /// Couche 10 (WAI) -- Registre de dictionnaires pour compression reseau
    /// statique. Le dictionnaire v5.0.0 est charge au demarrage et partage
    /// par tous les agents sur le meme serveur.
    pub wai_registry: Arc<wai::DictionaryRegistry>,
    /// TLS 1.3 (optionnel, mutuellement authentifie ou non) -- `None` par
    /// defaut (`try_with_data_path`), comportement inchange: TCP brut, meme
    /// qu'avant ce cablage. `start()` construit cette valeur depuis
    /// `CSTL_TLS_*` (voir plus bas) si `None` et que les variables
    /// d'environnement sont presentes; un test/smoke-test peut aussi
    /// l'assigner directement avant d'appeler `start()` (meme patron que
    /// `agent_registry`/`restricted_council`, voir les smoke-tests
    /// existants) pour utiliser des certificats generes en memoire sans
    /// passer par le disque.
    pub tls: Option<Arc<tls::TlsServer>>,
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
    /// Panique si l'ouverture/le chargement echoue -- pratique pour les tests et
    /// smoke-tests (base ":memory:" ou fichiers de test jetables, ou un echec
    /// DOIT arreter le test immediatement). Pour un vrai processus serveur
    /// (main.rs), preferer `try_with_data_path` et gerer l'erreur proprement:
    /// un `.expect()` ici produisait un panic Rust brut (backtrace, pas de
    /// message actionnable) sur un fichier SQLite corrompu/verrouille au
    /// demarrage -- trouvaille de l'audit du repo du 2026-09-04. Le
    /// comportement fail-fast (refuser de demarrer avec une base illisible)
    /// est correct et conserve ici; ce qui change, c'est que l'appelant peut
    /// maintenant choisir COMMENT il echoue.
    pub fn with_data_path(port: u16, data_path: &str) -> Self {
        match Self::try_with_data_path(port, data_path) {
            Ok(server) => server,
            Err(msg) => panic!("{msg}"),
        }
    }

    /// Meme construction que `with_data_path`, mais retourne un `Result` au
    /// lieu de paniquer -- permet a `main.rs` d'afficher un message clair et
    /// de sortir proprement (`std::process::exit`) plutot que de crasher avec
    /// un panic Rust brut sur une base SQLite corrompue/verrouillee.
    pub fn try_with_data_path(port: u16, data_path: &str) -> Result<Self, String> {
        let adn_store = AdnStore::open(data_path)
            .map_err(|e| format!("impossible d'ouvrir la base ADN '{data_path}': {e}"))?;
        // Seed la chaine en memoire depuis ce qui est deja persiste --
        // AVANT le 2026-09-04, ce chargement n'existait pas du tout:
        // `chain` demarrait toujours vide, meme quand cette meme base SQLite
        // contenait deja des payloads avec leur propre lignee de parent_hash.
        // Un redemarrage rompait donc silencieusement la continuite de la
        // chaine de hachage.
        let chain = adn_store
            .load_chain()
            .map_err(|e| format!("impossible de charger la chaine d'audit persistee depuis '{data_path}': {e}"))?;

        // Seed la Couche 2 (gouvernance) depuis ce qui est deja persiste --
        // meme logique que `chain` juste au-dessus. `since` ne rapatrie que
        // ce qui tombe encore dans la plus grande des deux fenetres
        // glissantes (DRIFT_WINDOW, 1h > BREAKER_WINDOW, 10 min) -- un
        // evenement plus vieux que ca n'aurait de toute facon plus aucun
        // effet sur `record()` (retire des le premier appel).
        let governance_since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            - crate::governance::DRIFT_WINDOW.as_secs() as i64;
        let governance_events = adn_store
            .load_governance_events(governance_since)
            .map_err(|e| format!("impossible de charger les evenements de gouvernance persistes depuis '{data_path}': {e}"))?;
        let governance_alerts = adn_store
            .load_governance_alerts()
            .map_err(|e| format!("impossible de charger les alertes de gouvernance persistees depuis '{data_path}': {e}"))?;
        let governance = GovernanceTracker::with_defaults_restored(&governance_events, &governance_alerts);

        // Initialise SigmaCalibrator pour standardisation EWMA inter-LLM
        // (Couche 5, Double Livre v5.2). Alpha = 0.2 pour adaptation rapide.
        let sigma_calibrator = SigmaCalibrator::new(0.2);

        // Initialise le registre WAI avec le dictionnaire statique v5.0.0
        let mut wai_registry = wai::DictionaryRegistry::new("cstl-v5.0.0".to_string());
        let standard_dict = wai::DictionaryVersion::new_standard_cstl_v5_0_0();
        let dict_hash = standard_dict.version_hash.clone();
        let dict_timestamp = standard_dict.timestamp;
        let dict_symbols_json = serde_json::to_string(&standard_dict.symbols)
            .unwrap_or_default();
        let dict_size = standard_dict.size_bytes;

        // Enregistre le dictionnaire dans la memoire du serveur
        wai_registry.register_version(standard_dict.clone());

        // Persiste le dictionnaire dans la base ADN pour les demarrages futurs
        adn_store
            .save_wai_dictionary(&dict_hash, dict_timestamp, &dict_symbols_json, dict_size)
            .map_err(|e| format!("impossible de sauvegarder le dictionnaire WAI v5.0.0: {e}"))?;

        let adn_store_arc = Arc::new(Mutex::new(adn_store));
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
        let restricted_council_arc = Arc::new(RestrictedCouncil::from_env());
        let governance_arc = Arc::new(Mutex::new(governance));
        // Couche 9 (2026-10-01) -- construit ici (pas dans `start()`) pour
        // que meme un appelant qui ne lance jamais `start()` (tests,
        // smoke-tests qui construisent `ServerContext` a la main) ait un
        // orchestrateur fonctionnel; les regles par defaut, elles, sont
        // enregistrees dans `start()` (operation async, voir plus bas).
        let deontic_arc = Arc::new(deontic_orchestration::DeonticOrchestrator::new(
            256,
            adn_store_arc.clone(),
            governance_arc.clone(),
            restricted_council_arc.clone(),
        ));
        Ok(CstlNativeServer {
            port,
            agent_registry: Arc::new(RwLock::new(AgentRegistry::new())),
            chain: Arc::new(Mutex::new(chain)),
            kb_verifier: Arc::new(KbVerifier::new()),
            adn_store: adn_store_arc,
            restricted_council: restricted_council_arc,
            // None si TELEGRAM_BOT_TOKEN / TELEGRAM_CHAT_ID absents de l'environnement -
            // degradation propre, le serveur marche pareil sans notification.
            telegram: TelegramNotifier::from_env().map(Arc::new),
            // None si OBSIDIAN_VAULT_PATH absent de l'environnement - degradation
            // propre, le serveur marche pareil sans escalade Obsidian.
            obsidian: ObsidianEscalation::from_env().map(Arc::new),
            governance: governance_arc,
            sigma_calibrator: Arc::new(Mutex::new(sigma_calibrator)),
            deontic: deontic_arc,
            wai_registry: Arc::new(wai_registry),
            tls: None,
        })
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("[CSTL-Native Server] Starting on port {}", self.port);

        // Couche 9 -- regles par defaut, enregistrees une seule fois au
        // demarrage (register_rule est async, donc pas possible depuis le
        // constructeur sync `try_with_data_path`). Toutes MUST/MAY -- aucune
        // MUST_NOT par defaut: le but ici est d'avoir un journal observable
        // des agent_register/rulings/breaches reels des la premiere requete,
        // pas de bloquer quoi que ce soit (voir le commentaire sur le champ
        // `deontic` de `ServerContext` pour la limite honnete de ce
        // cablage: observation/audit, pas garde preventive).
        self.deontic.register_rule(deontic_orchestration::DeonticRule::new_must(
            "agent_register", "all_agents", "log_agent_registration",
        )).await;
        self.deontic.register_rule(deontic_orchestration::DeonticRule::new_must(
            "arbitration_ruling", "all_rulings", "log_ruling_applied",
        )).await;
        self.deontic.register_rule(deontic_orchestration::DeonticRule::new_may(
            "governance_breach", "all_breaches", "log_governance_breach",
        )).await;
        eprintln!("[CSTL-Native Server] Deontic orchestrator actif ({} regles par defaut)", self.deontic.rules_count().await);

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

        // TLS 1.3 (optionnel, mutuellement authentifie ou non) -- 2026-10-01,
        // voir server/tls.rs. Priorite: `self.tls` (deja construit, ex. par
        // un smoke-test avec des certificats generes en memoire) > variables
        // d'environnement `CSTL_TLS_*` (chargees UNE SEULE FOIS ici, meme
        // discipline que CSTL_COLLECT_RESPONSE_CORPUS juste plus bas) > rien
        // (TCP brut, comportement inchange -- c'est l'etat par defaut tant
        // qu'aucune des deux sources n'est fournie).
        let tls_server: Option<Arc<tls::TlsServer>> = if let Some(tls) = &self.tls {
            eprintln!("[CSTL-Native Server] TLS 1.3 actif (configure directement sur CstlNativeServer.tls)");
            Some(tls.clone())
        } else {
            match (std::env::var("CSTL_TLS_CERT_PATH"), std::env::var("CSTL_TLS_KEY_PATH")) {
                (Ok(cert_path), Ok(key_path)) => {
                    let cert_chain_pem = std::fs::read(&cert_path)
                        .map_err(|e| format!("lecture de CSTL_TLS_CERT_PATH='{}' echouee: {}", cert_path, e))?;
                    let private_key_pem = std::fs::read(&key_path)
                        .map_err(|e| format!("lecture de CSTL_TLS_KEY_PATH='{}' echouee: {}", key_path, e))?;
                    let require_mutual_auth = std::env::var("CSTL_TLS_REQUIRE_MUTUAL_AUTH")
                        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                        .unwrap_or(false);
                    let client_ca_pem = match std::env::var("CSTL_TLS_CLIENT_CA_PATH") {
                        Ok(ca_path) => Some(
                            std::fs::read(&ca_path)
                                .map_err(|e| format!("lecture de CSTL_TLS_CLIENT_CA_PATH='{}' echouee: {}", ca_path, e))?,
                        ),
                        Err(_) => None,
                    };
                    let tls_config = tls::TlsConfig {
                        cert_chain_pem,
                        private_key_pem,
                        client_ca_pem,
                        require_mutual_auth,
                    };
                    let tls_server = tls::TlsServer::new(tls_config)
                        .map_err(|e| format!("configuration TLS invalide (CSTL_TLS_*): {}", e))?;
                    eprintln!(
                        "[CSTL-Native Server] TLS 1.3 actif (CSTL_TLS_CERT_PATH/CSTL_TLS_KEY_PATH, mutual_auth={})",
                        require_mutual_auth
                    );
                    Some(Arc::new(tls_server))
                }
                _ => {
                    eprintln!("[CSTL-Native Server] TLS desactive (CSTL_TLS_CERT_PATH / CSTL_TLS_KEY_PATH absents) -- TCP brut");
                    None
                }
            }
        };
        let tls_acceptor = tls_server.map(|s| Arc::new(s.acceptor()));

        let ctx = ServerContext {
            agent_registry: self.agent_registry.clone(),
            chain: self.chain.clone(),
            kb_verifier: self.kb_verifier.clone(),
            adn_store: self.adn_store.clone(),
            restricted_council: self.restricted_council.clone(),
            telegram: self.telegram.clone(),
            obsidian: self.obsidian.clone(),
            governance: self.governance.clone(),
            sigma_calibrator: self.sigma_calibrator.clone(),
            deontic: self.deontic.clone(),
            // Lu UNE fois ici (pas a chaque requete): la collecte est une
            // decision de deploiement, pas quelque chose qui doit changer
            // en cours de route sans redemarrer le serveur.
            collect_response_corpus: std::env::var("CSTL_COLLECT_RESPONSE_CORPUS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        };

        if ctx.collect_response_corpus {
            eprintln!("[CSTL-Native Server] Collecte du corpus de reponses ACTIVE (CSTL_COLLECT_RESPONSE_CORPUS) -- chaque reponse plain-text sera persistee dans response_corpus");
        }

        listener::accept_connections(listener, Arc::new(ctx), tls_acceptor).await?;

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
