//! Couche 5c: REST API Server for Audit Trail Access
//! Provides HTTP endpoints for ADN store audit trail querying via axum
//! Runs alongside TCP server on separate port (default 8000)

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::adn_store::AdnStore;
use crate::server::audit;
use crate::server::graphify_server::GraphifyExporter;
use crate::server::deontic_orchestration::DeonticOrchestrator;
use crate::server::wai::DictionaryRegistry;
use crate::calibration::SigmaCalibrator;

/// Audit entry for API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiAuditEntry {
    pub seq: u64,
    pub hash: String,
    pub parent_hash: String,
    pub sender: String,
    pub receiver: String,
    pub purpose: String,
}

/// Case information for API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiCaseInfo {
    pub case_id: String,
    pub initiator: String,
    pub subject: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Council log entry for API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiCouncilLogEntry {
    pub id: i64,
    pub hash: String,
    pub action: String,
    pub by_whom: String,
    pub note: Option<String>,
    pub timestamp: i64,
}

/// Ruling information for API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiRulingInfo {
    pub ruling_id: String,
    pub ruling_text: String,
    pub decided_by: String,
    pub status: String,
    pub created_at: i64,
}

/// Complete audit trail response for a case
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuditTrailResponse {
    pub case_info: Option<ApiCaseInfo>,
    pub audit_entries: Vec<ApiAuditEntry>,
    pub council_logs: Vec<ApiCouncilLogEntry>,
    pub ruling_info: Option<ApiRulingInfo>,
    pub total_entries: usize,
}

/// Query filter parameters
#[derive(Debug, Deserialize)]
pub struct AuditQueryParams {
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    pub entity: Option<String>,
    pub action: Option<String>,
    pub purpose: Option<String>,
    pub limit: Option<usize>,
}

/// Audit query response
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditQueryResponse {
    pub results: Vec<serde_json::Value>,
    pub total_count: usize,
    pub query_params: serde_json::Value,
}

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub timestamp: String,
    pub database: HealthCheckDatabaseInfo,
}

/// Database info for health check
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthCheckDatabaseInfo {
    pub connected: bool,
    pub audit_trail_entries: u64,
}

/// Statistics response
#[derive(Debug, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_entries: u64,
    pub total_cases: u64,
    pub timestamp: String,
}

/// API state containing shared ADN store
///
/// `chain` ajoute le 2026-10-01: route `/graphify/*` (Couche 6, voir
/// `graphify_server.rs`), trouve dead-code par le meme grep que CASTLE/
/// quorum/tls -- a la difference de tls.rs, c'etait un module REEL
/// (construit un graphe de noeuds/arcs depuis la chaine d'audit, deja
/// teste en isolation) simplement jamais appele: `pub mod graphify_server;`
/// seul, aucune route REST, malgre le README qui affirmait depuis un
/// certain temps "GET /graphify/export" "v5.1 COMPLETE" -- affirmation
/// fausse avant ce commit, corrigee ici (voir README).
/// `deontic` ajoute le 2026-10-01: route `/deontic/*` (Couche 9, voir
/// `deontic_orchestration.rs`) -- meme histoire que `chain`/graphify
/// ci-dessus: module reel (15 tests d'integration passent), trouve dead
/// code, jamais construit nulle part avant ce commit, malgre le README qui
/// affirmait "v5.1 COMPLETE". Expose ici en lecture seule (executions +
/// nombre de regles) pour verification live -- `DeonticOrchestrator` est
/// deja `Clone`-free (methodes `&self` async derriere des `Mutex`
/// internes), donc un seul `Arc` partage suffit, pas de `Mutex` externe
/// necessaire ici contrairement a `chain`/`adn_store`.
/// `wai_registry`/`sigma_calibrator` ajoutes le 2026-10-01: routes
/// `/wai/*` et `/calibration/*`, meme histoire que `chain`/`deontic`
/// ci-dessus -- trouvailles de l'audit "pub fn jamais appelees": les deux
/// etaient deja construits et partages sur `ServerContext` (voir
/// `server/mod.rs`), mais rien ne les lisait jamais apres construction.
/// `wai_registry` est deja un `Arc<DictionaryRegistry>` sans `Mutex`
/// interne (lecture seule une fois demarre, un seul writer a l'init) --
/// partage direct. `sigma_calibrator` est deja `Arc<Mutex<...>>` sur le
/// contexte, reutilise tel quel.
#[derive(Clone)]
pub struct ApiState {
    pub adn_store: Arc<Mutex<AdnStore>>,
    pub chain: Arc<Mutex<audit::HashChain>>,
    pub deontic: Arc<DeonticOrchestrator>,
    pub wai_registry: Arc<DictionaryRegistry>,
    pub sigma_calibrator: Arc<Mutex<SigmaCalibrator>>,
}

/// Health check endpoint
async fn health_check(State(state): State<ApiState>) -> impl IntoResponse {
    let adn_store = state.adn_store.lock().await;
    match adn_store.stats() {
        Ok(stats) => {
            let response = json!({
                "status": "healthy",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "database": {
                    "connected": true,
                    "audit_trail_entries": stats.total
                }
            });
            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            let response = json!({
                "status": "error",
                "error": format!("Database error: {}", e)
            });
            (StatusCode::SERVICE_UNAVAILABLE, Json(response))
        }
    }
}

/// Get audit trail for a specific case
async fn get_audit_trail(
    State(state): State<ApiState>,
    Path(case_id): Path<String>,
) -> impl IntoResponse {
    let adn_store = state.adn_store.lock().await;

    // Query arbitrage case
    match adn_store.get_arbitrage_case(&case_id) {
        Ok(Some(case)) => {
            let case_info = ApiCaseInfo {
                case_id: case.case_id.clone(),
                initiator: case.initiator.clone(),
                subject: case.subject.clone(),
                status: case.status.clone(),
                created_at: case.created_at,
                updated_at: case.updated_at,
            };

            // Query council logs for this case
            let council_logs = match adn_store.council_log_for(&case_id) {
                Ok(logs) => logs
                    .iter()
                    .map(|log| ApiCouncilLogEntry {
                        id: log.id,
                        hash: log.hash.clone(),
                        action: log.action.clone(),
                        by_whom: log.by_whom.clone(),
                        note: log.note.clone(),
                        timestamp: log.timestamp,
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };

            let response = json!({
                "case_info": case_info,
                "audit_entries": [],
                "council_logs": council_logs,
                "ruling_info": null,
                "total_entries": council_logs.len()
            });

            (StatusCode::OK, Json(response))
        }
        Ok(None) => {
            let response = json!({
                "error": format!("Case {} not found", case_id)
            });
            (StatusCode::NOT_FOUND, Json(response))
        }
        Err(e) => {
            let response = json!({
                "error": format!("Database error: {}", e)
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    }
}

/// Query audit trail with filters
async fn query_audit_trail(
    State(_state): State<ApiState>,
    Query(params): Query<AuditQueryParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100).min(1000);

    let response = json!({
        "results": [],
        "total_count": 0,
        "query_params": {
            "start_date": params.start_date,
            "end_date": params.end_date,
            "entity": params.entity,
            "action": params.action,
            "purpose": params.purpose,
            "limit": limit
        }
    });

    (StatusCode::OK, Json(response))
}

/// Get audit statistics
async fn get_stats(State(state): State<ApiState>) -> impl IntoResponse {
    let adn_store = state.adn_store.lock().await;

    match adn_store.stats() {
        Ok(stats) => {
            let response = json!({
                "total_entries": stats.total,
                "committed_entries": stats.committed,
                "pending_entries": stats.pending,
                "timestamp": chrono::Utc::now().to_rfc3339()
            });
            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            let response = json!({
                "error": format!("Database error: {}", e)
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    }
}

/// `GET /graphify/export` -- construit et retourne le graphe complet
/// (noeuds agents + entrees d'audit, arcs sends_to/responds_to) depuis la
/// chaine d'audit COURANTE. Clone la chaine (`HashChain: Clone`, voir
/// `audit.rs`) pendant la duree du verrou plutot que de retenir le lock
/// pendant toute la construction du `GraphifyPayload` -- cette derniere
/// n'a besoin d'aucun etat partage apres le snapshot.
async fn graphify_export(State(state): State<ApiState>) -> impl IntoResponse {
    let chain_snapshot = { state.chain.lock().await.clone() };
    let exporter = GraphifyExporter::new(chain_snapshot);
    let payload = exporter.build_from_audit_trail();
    Json(payload)
}

/// `GET /graphify/stats` -- complement naturel de `/graphify/export`
/// (comptes de noeuds/arcs par type) -- construit le meme graphe puis
/// delegue a `GraphifyExporter::get_graph_stats`, deja teste en isolation
/// mais jusqu'ici jamais appele depuis une route reelle.
async fn graphify_stats(State(state): State<ApiState>) -> impl IntoResponse {
    let chain_snapshot = { state.chain.lock().await.clone() };
    let exporter = GraphifyExporter::new(chain_snapshot);
    let payload = exporter.build_from_audit_trail();
    let stats = exporter.get_graph_stats(&payload);
    Json(stats)
}

/// `GET /deontic/executions` -- historique des executions de regles
/// deontiques (Couche 9) accumulees depuis le demarrage du serveur (en
/// memoire seulement, voir `DeonticOrchestrator` -- aucune persistance
/// SQLite pour l'instant, limite honnete a documenter comme pour le reste
/// de cette couche).
async fn deontic_executions(State(state): State<ApiState>) -> impl IntoResponse {
    let executions = state.deontic.get_executions().await;
    let rules_count = state.deontic.rules_count().await;
    Json(json!({
        "rules_count": rules_count,
        "execution_count": executions.len(),
        "executions": executions,
    }))
}

/// Query params partages par les routes de requete Graphify.
#[derive(Debug, Deserialize)]
pub struct GraphifyQueryParams {
    pub node_type: Option<String>,
    pub edge_type: Option<String>,
    pub q: Option<String>,
    pub start: Option<String>,
    pub depth: Option<usize>,
}

/// `GET /graphify/filter?node_type=...&edge_type=...` -- filtre le graphe
/// courant par type de noeud et/ou d'arc. Trouvaille de l'audit "pub fn
/// jamais appelees" (2026-10-01): `filter_nodes_by_type`/
/// `filter_edges_by_type` existaient depuis la creation de ce module mais
/// n'avaient aucune route -- seuls `/export` et `/stats` l'etaient.
async fn graphify_filter(State(state): State<ApiState>, Query(params): Query<GraphifyQueryParams>) -> impl IntoResponse {
    let chain_snapshot = { state.chain.lock().await.clone() };
    let exporter = GraphifyExporter::new(chain_snapshot);
    let payload = exporter.build_from_audit_trail();

    let nodes = params.node_type.as_deref().map(|t| exporter.filter_nodes_by_type(&payload, t));
    let edges = params.edge_type.as_deref().map(|t| exporter.filter_edges_by_type(&payload, t));

    Json(json!({
        "nodes": nodes.unwrap_or_default(),
        "edges": edges.unwrap_or_default(),
    }))
}

/// `GET /graphify/search?q=...` -- recherche texte sur label/id/metadata
/// des noeuds. Meme trouvaille: `search_nodes` jamais appelee avant ce
/// commit.
async fn graphify_search(State(state): State<ApiState>, Query(params): Query<GraphifyQueryParams>) -> impl IntoResponse {
    let chain_snapshot = { state.chain.lock().await.clone() };
    let exporter = GraphifyExporter::new(chain_snapshot);
    let payload = exporter.build_from_audit_trail();

    match params.q {
        Some(q) => Json(json!({ "nodes": exporter.search_nodes(&payload, &q) })).into_response(),
        None => (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing query param 'q'" }))).into_response(),
    }
}

/// `GET /graphify/traverse?start=<node_id>&depth=<n>` -- sous-graphe
/// atteignable depuis `start` en au plus `depth` sauts (BFS). Meme
/// trouvaille: `traverse_graph` jamais appelee avant ce commit.
async fn graphify_traverse(State(state): State<ApiState>, Query(params): Query<GraphifyQueryParams>) -> impl IntoResponse {
    let chain_snapshot = { state.chain.lock().await.clone() };
    let exporter = GraphifyExporter::new(chain_snapshot);
    let payload = exporter.build_from_audit_trail();

    match params.start {
        Some(start) => {
            let depth = params.depth.unwrap_or(2);
            Json(exporter.traverse_graph(&payload, &start, depth)).into_response()
        }
        None => (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing query param 'start'" }))).into_response(),
    }
}

/// `GET /wai/dictionaries` -- liste les versions de dictionnaire WAI
/// enregistrees (resume: hash, timestamp, nb symboles, taille). Trouvaille
/// de l'audit "pub fn jamais appelees" (2026-10-01): `wai_registry` est
/// construit au demarrage et une version y est enregistree
/// (`server/mod.rs`), mais rien ne le relisait jamais avant ce commit --
/// `list_versions`/`get_latest`/`stats` existaient sans aucune route.
async fn wai_list_dictionaries(State(state): State<ApiState>) -> impl IntoResponse {
    Json(state.wai_registry.list_versions())
}

/// `GET /wai/dictionaries/latest` -- derniere version enregistree
/// (dictionnaire complet, pas seulement le resume).
async fn wai_latest_dictionary(State(state): State<ApiState>) -> impl IntoResponse {
    match state.wai_registry.get_latest() {
        Some(dict) => Json(dict.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "no dictionary version registered" }))).into_response(),
    }
}

/// `GET /wai/stats` -- statistiques du registre (nb versions, nb symboles
/// total, taille totale).
async fn wai_stats(State(state): State<ApiState>) -> impl IntoResponse {
    Json(state.wai_registry.stats())
}

/// `GET /calibration/agents` -- snapshot de toutes les calibrations EWMA
/// par agent (monitoring/debug). Trouvaille de l'audit "pub fn jamais
/// appelees" (2026-10-01): `sigma_calibrator` est deja partage sur
/// `ServerContext` et alimente en continu par le pipeline de validation,
/// mais `get_all_calibrations`/`get_calibration` n'avaient aucune route
/// pour inspecter l'etat accumule depuis l'exterieur.
async fn calibration_all(State(state): State<ApiState>) -> impl IntoResponse {
    let calibrator = state.sigma_calibrator.lock().await;
    Json(calibrator.get_all_calibrations())
}

/// `GET /calibration/agents/:agent_name` -- calibration EWMA d'un agent
/// specifique.
async fn calibration_for_agent(State(state): State<ApiState>, Path(agent_name): Path<String>) -> impl IntoResponse {
    let calibrator = state.sigma_calibrator.lock().await;
    match calibrator.get_calibration(&agent_name) {
        Some(c) => Json(c.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": format!("no calibration recorded for agent '{}'", agent_name) }))).into_response(),
    }
}

/// Root endpoint
async fn root() -> impl IntoResponse {
    let response = json!({
        "service": "CSTL Audit Trail Server (Rust)",
        "version": "5.1.0",
        "endpoints": {
            "health": "GET /health",
            "audit_trail": "GET /audit/{case_id}",
            "query_audit": "POST /audit/query",
            "stats": "GET /audit/stats",
            "graphify_export": "GET /graphify/export",
            "graphify_stats": "GET /graphify/stats",
            "graphify_filter": "GET /graphify/filter?node_type=...&edge_type=...",
            "graphify_search": "GET /graphify/search?q=...",
            "graphify_traverse": "GET /graphify/traverse?start=...&depth=...",
            "deontic_executions": "GET /deontic/executions",
            "wai_dictionaries": "GET /wai/dictionaries",
            "wai_latest_dictionary": "GET /wai/dictionaries/latest",
            "wai_stats": "GET /wai/stats",
            "calibration_all": "GET /calibration/agents",
            "calibration_for_agent": "GET /calibration/agents/{agent_name}"
        }
    });

    Json(response)
}

/// Create and return the REST API router
pub fn create_router(
    adn_store: Arc<Mutex<AdnStore>>,
    chain: Arc<Mutex<audit::HashChain>>,
    deontic: Arc<DeonticOrchestrator>,
    wai_registry: Arc<DictionaryRegistry>,
    sigma_calibrator: Arc<Mutex<SigmaCalibrator>>,
) -> Router {
    let state = ApiState { adn_store, chain, deontic, wai_registry, sigma_calibrator };

    Router::new()
        .route("/", get(root))
        .route("/health", get(health_check))
        .route("/audit/:case_id", get(get_audit_trail))
        .route("/audit/query", post(query_audit_trail))
        .route("/audit/stats", get(get_stats))
        .route("/graphify/export", get(graphify_export))
        .route("/graphify/stats", get(graphify_stats))
        .route("/graphify/filter", get(graphify_filter))
        .route("/graphify/search", get(graphify_search))
        .route("/graphify/traverse", get(graphify_traverse))
        .route("/deontic/executions", get(deontic_executions))
        .route("/wai/dictionaries", get(wai_list_dictionaries))
        .route("/wai/dictionaries/latest", get(wai_latest_dictionary))
        .route("/wai/stats", get(wai_stats))
        .route("/calibration/agents", get(calibration_all))
        .route("/calibration/agents/:agent_name", get(calibration_for_agent))
        .with_state(state)
}

/// Start REST API server on specified port
pub async fn start_rest_api(
    adn_store: Arc<Mutex<AdnStore>>,
    chain: Arc<Mutex<audit::HashChain>>,
    deontic: Arc<DeonticOrchestrator>,
    wai_registry: Arc<DictionaryRegistry>,
    sigma_calibrator: Arc<Mutex<SigmaCalibrator>>,
    host: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let router = create_router(adn_store, chain, deontic, wai_registry, sigma_calibrator);
    let addr_str = format!("{}:{}", host, port);
    let addr: std::net::SocketAddr = addr_str.parse()?;

    eprintln!("✅ REST API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router.into_make_service(),
    )
    .await?;

    Ok(())
}
