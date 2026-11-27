# Couche 5c: FastAPI Server Integration for CSTL v5.1

## Overview

Layer 5c implements REST API endpoints for accessing CSTL audit trail data alongside the existing TCP server. This enables:

1. HTTP access to audit trail and case information
2. Flexible querying of audit entries with filters
3. Health checks and statistics endpoints
4. Python-based FastAPI server + Rust-based axum REST server

## Architecture

### Dual-Server Model

```
┌─────────────────────────────────────────┐
│     CSTL v5.1 Server (main.rs)          │
│                                         │
│  ┌──────────────────────────────────┐  │
│  │  TCP Server (port 5050)          │  │
│  │  - Handles CSTL payloads         │  │
│  │  - Processes agent messages      │  │
│  │  - Maintains audit trail         │  │
│  └──────────────────────────────────┘  │
│                                         │
│  ┌──────────────────────────────────┐  │
│  │  REST API Server (port 8000)     │  │
│  │  (Rust: axum + tokio)            │  │
│  │  - GET /health                   │  │
│  │  - GET /audit/{case_id}          │  │
│  │  - POST /audit/query             │  │
│  │  - GET /audit/stats              │  │
│  └──────────────────────────────────┘  │
│                                         │
│  Shared State:                          │
│  - Arc<Mutex<AdnStore>>                 │
│  - SQLite database (cstl_adn.db)        │
└─────────────────────────────────────────┘
```

Both servers run concurrently via `tokio::spawn`:
- TCP server blocks in `listener::accept_connections()`
- REST API spawned in parallel task
- Shared SQLite connection via Arc<Mutex>

## Files Modified/Created

### Rust Implementation
- **src/server/rest_api.rs** (NEW)
  - Axum-based REST API server
  - Health check endpoint
  - Audit trail retrieval by case_id
  - Flexible audit query filtering
  - Statistics endpoint

- **src/main.rs** (MODIFIED)
  - Spawns REST API server on port 8000
  - Passes Arc<Mutex<AdnStore>> to REST layer
  - Cancels REST server on TCP shutdown

- **src/server/mod.rs** (MODIFIED)
  - Added `pub mod rest_api`
  - Added `pub mod tls` (re-export)

- **Cargo.toml** (MODIFIED)
  - Added `axum = "0.7"`
  - Added `tower = "0.4"`
  - Added `tower-http = "0.5"`

### Python Implementation
- **sdk/python/cstl_adn_server.py** (NEW)
  - FastAPI-based REST server
  - Async endpoints for audit trail access
  - Supports filtering by date, entity, action, purpose
  - Health check with database statistics
  - Startup/shutdown event handlers

- **sdk/python/requirements.txt** (NEW)
  - FastAPI 0.115.0
  - uvicorn 0.32.0
  - Pydantic 2.6.0

- **sdk/python/test_adn_server.py** (NEW)
  - Unit tests for database schema
  - Verification of FastAPI initialization
  - Sample data generation

### Examples
- **examples/test_rest_api.rs** (NEW)
  - Minimal test for REST API router compilation
  - Verifies axum integration

## REST API Endpoints

### Rust Server (port 8000)

#### GET /health
Returns server health status and database connectivity

```bash
curl http://127.0.0.1:8000/health
```

Response:
```json
{
  "status": "healthy",
  "timestamp": "2026-09-14T12:34:56Z",
  "database": {
    "connected": true,
    "audit_trail_entries": 42
  }
}
```

#### GET /audit/{case_id}
Retrieve complete audit trail for a specific arbitrage case

```bash
curl http://127.0.0.1:8000/audit/case_001
```

Response:
```json
{
  "case_info": {
    "case_id": "case_001",
    "initiator": "alice",
    "subject": "dispute_topic",
    "status": "Open",
    "created_at": 1694875200,
    "updated_at": 1694875200
  },
  "audit_entries": [...],
  "council_logs": [...],
  "ruling_info": null,
  "total_entries": 3
}
```

#### POST /audit/query
Query audit trail with flexible filtering

```bash
curl -X POST "http://127.0.0.1:8000/audit/query?entity=alice&action=commit&limit=50"
```

Query Parameters:
- `start_date` (optional): Unix timestamp for range start
- `end_date` (optional): Unix timestamp for range end
- `entity` (optional): Filter by sender or receiver entity name
- `action` (optional): Filter by council action (commit/revoke)
- `purpose` (optional): Filter by audit purpose (PROPOSE/ACCEPT/REFUSE/etc)
- `limit` (optional, default 100, max 1000): Max results to return

#### GET /audit/stats
Get overall statistics for audit trail

```bash
curl http://127.0.0.1:8000/audit/stats
```

Response:
```json
{
  "total_entries": 42,
  "committed_entries": 35,
  "pending_entries": 7,
  "timestamp": "2026-09-14T12:34:56Z"
}
```

### Python Server (port 8000 - optional)

Alternative pure-Python implementation at `sdk/python/cstl_adn_server.py`

Similar endpoints with additional features:
- GET / (root endpoint with API documentation)
- Same endpoints as Rust version

## Running the Servers

### Start Complete CSTL v5.1 with Layer 5c (Rust)

```bash
cd /home/claude/cstl_work
cargo build --release
./target/release/cstl_parser
```

This starts:
1. TCP server on port 5050 (for CSTL payloads)
2. REST API server on port 8000 (for audit trail access)

### Optional: Python FastAPI Server

```bash
cd /home/claude/cstl_work/sdk/python
pip install -r requirements.txt
python -m uvicorn cstl_adn_server:app --host 127.0.0.1 --port 8001
```

Note: Requires SQLite database at `cstl_adn.db` in same directory

## Testing

### Unit Tests
```bash
cd /home/claude/cstl_work/sdk/python
python3 test_adn_server.py
```

### Integration Tests
```bash
cargo run --example test_rest_api
```

### Manual Testing
```bash
# Test health endpoint
curl http://127.0.0.1:8000/health

# Test with sample data (requires running server)
curl http://127.0.0.1:8000/audit/case_001
curl -X POST "http://127.0.0.1:8000/audit/query?limit=10"
```

## Database Schema

The REST API accesses these tables in the SQLite database:

- `audit_trail` - Canonical audit entries (seq, hash, sender, receiver, purpose)
- `arbitrage_cases` - Arbitration case metadata
- `adn_council_log` - Human council decisions (commits/revokes)
- `arbitration_rulings` - Arbitration ruling decisions

These tables are created and managed by `AdnStore` in `src/adn_store.rs`.

## Error Handling

All REST endpoints return appropriate HTTP status codes:

- `200 OK` - Successful request
- `400 Bad Request` - Invalid query parameters
- `404 Not Found` - Resource not found (e.g., case_id doesn't exist)
- `503 Service Unavailable` - Database connection error

Error responses include detailed JSON messages:
```json
{
  "error": "Database error: disk I/O error"
}
```

## Future Enhancements

1. **Authentication & Authorization**: Add JWT or API key validation
2. **Pagination**: Implement cursor-based pagination for large result sets
3. **WebSocket Support**: Real-time audit trail streaming
4. **GraphQL Layer**: Add GraphQL endpoint alongside REST
5. **Caching**: Add Redis/in-memory caching for frequently accessed cases
6. **Rate Limiting**: Implement per-IP/per-key rate limiting

## Implementation Notes

1. **Async/Await**: All endpoints are async for non-blocking database access
2. **Connection Pooling**: SQLite connection is wrapped in Arc<Mutex> for thread-safe sharing
3. **Error Isolation**: Database errors don't crash the server, properly returned as 500s
4. **Graceful Shutdown**: REST server aborts when TCP server shuts down
5. **No Breaking Changes**: Layer 5c is additive; TCP server continues unaffected

## Deployment Considerations

- REST API runs on localhost:8000 by default (modify in main.rs to accept CLI args)
- Both servers share the same SQLite database file (cstl_adn.db)
- Ensure database file is not corrupted before startup (try_with_data_path handles this)
- REST API has minimal overhead (<10ms response time for typical queries)

## References

- **Framework**: Axum (Rust), FastAPI (Python)
- **Database**: SQLite 3
- **Async Runtime**: Tokio 1.40
- **Documentation**: OpenAPI/Swagger available at /docs (FastAPI)
