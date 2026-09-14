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
#[derive(Clone)]
pub struct ApiState {
    pub adn_store: Arc<Mutex<AdnStore>>,
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

/// Root endpoint
async fn root() -> impl IntoResponse {
    let response = json!({
        "service": "CSTL Audit Trail Server (Rust)",
        "version": "5.1.0",
        "endpoints": {
            "health": "GET /health",
            "audit_trail": "GET /audit/{case_id}",
            "query_audit": "POST /audit/query",
            "stats": "GET /audit/stats"
        }
    });

    Json(response)
}

/// Create and return the REST API router
pub fn create_router(adn_store: Arc<Mutex<AdnStore>>) -> Router {
    let state = ApiState { adn_store };

    Router::new()
        .route("/", get(root))
        .route("/health", get(health_check))
        .route("/audit/:case_id", get(get_audit_trail))
        .route("/audit/query", post(query_audit_trail))
        .route("/audit/stats", get(get_stats))
        .with_state(state)
}

/// Start REST API server on specified port
pub async fn start_rest_api(
    adn_store: Arc<Mutex<AdnStore>>,
    host: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let router = create_router(adn_store);
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
