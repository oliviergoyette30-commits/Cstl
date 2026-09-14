#!/usr/bin/env python3
"""
Couche 5c: FastAPI REST Server for CSTL Audit Trail
Exposes ADN store audit trail via HTTP endpoints
"""

import sqlite3
import json
from datetime import datetime
from typing import Optional, List, Dict, Any
from fastapi import FastAPI, HTTPException, Query
from fastapi.responses import JSONResponse
import uvicorn
import asyncio
import logging

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = FastAPI(
    title="CSTL Audit Trail Server",
    version="5.1.0",
    description="REST API for CSTL ADN Store Audit Trail Access"
)

# Global database connection
DB_PATH = "cstl_adn.db"
conn = None


def get_db_connection():
    """Get or create database connection"""
    global conn
    if conn is None:
        conn = sqlite3.connect(DB_PATH, check_same_thread=False)
        conn.row_factory = sqlite3.Row
    return conn


@app.on_event("startup")
async def startup():
    """Initialize database connection on startup"""
    logger.info(f"Connecting to ADN database: {DB_PATH}")
    try:
        db = get_db_connection()
        db.execute("PRAGMA foreign_keys = ON;")
        # Test connection
        db.execute("SELECT COUNT(*) FROM audit_trail")
        logger.info("✓ Database connected and audit_trail table accessible")
    except sqlite3.Error as e:
        logger.error(f"Database connection error: {e}")
        raise


@app.on_event("shutdown")
async def shutdown():
    """Close database connection on shutdown"""
    global conn
    if conn:
        conn.close()
        conn = None
        logger.info("Database connection closed")


@app.get("/health", tags=["Health"])
async def health_check() -> Dict[str, Any]:
    """
    Health check endpoint - verifies database connectivity

    Returns:
        status: "healthy" if system is operational
        timestamp: current UTC timestamp
        database: database connection status
    """
    try:
        db = get_db_connection()
        # Test database connectivity
        cursor = db.execute("SELECT COUNT(*) as count FROM audit_trail")
        result = cursor.fetchone()
        audit_count = result["count"] if result else 0

        return {
            "status": "healthy",
            "timestamp": datetime.utcnow().isoformat(),
            "database": {
                "connected": True,
                "audit_trail_entries": audit_count
            }
        }
    except Exception as e:
        logger.error(f"Health check failed: {e}")
        raise HTTPException(status_code=503, detail=str(e))


@app.get("/audit/{case_id}", tags=["Audit"])
async def get_audit_trail(case_id: str) -> Dict[str, Any]:
    """
    Retrieve complete audit trail for a specific case

    Args:
        case_id: The arbitrage case ID to query

    Returns:
        case_info: Arbitrage case details
        audit_entries: List of audit trail entries (ordered by sequence)
        council_logs: List of council decision logs for the case
        ruling_info: Arbitration ruling (if exists)
        total_entries: Count of audit entries
    """
    try:
        db = get_db_connection()

        # Get arbitrage case info
        case_cursor = db.execute(
            "SELECT case_id, initiator, subject, status, created_at, updated_at FROM arbitrage_cases WHERE case_id = ?",
            (case_id,)
        )
        case_row = case_cursor.fetchone()

        if not case_row:
            raise HTTPException(status_code=404, detail=f"Case {case_id} not found")

        case_info = {
            "case_id": case_row["case_id"],
            "initiator": case_row["initiator"],
            "subject": case_row["subject"],
            "status": case_row["status"],
            "created_at": case_row["created_at"],
            "updated_at": case_row["updated_at"]
        }

        # Get all audit entries related to this case (via hash linkage)
        # First, find all hashes mentioned in adn_store for this case
        audit_entries = []
        cursor = db.execute(
            """SELECT seq, hash, parent_hash, sender, receiver, purpose
               FROM audit_trail
               WHERE sender LIKE ? OR receiver LIKE ?
               ORDER BY seq ASC""",
            (f"%{case_id}%", f"%{case_id}%")
        )

        for row in cursor.fetchall():
            audit_entries.append({
                "seq": row["seq"],
                "hash": row["hash"],
                "parent_hash": row["parent_hash"],
                "sender": row["sender"],
                "receiver": row["receiver"],
                "purpose": row["purpose"]
            })

        # Get council logs for related hashes
        council_logs = []
        if audit_entries:
            hash_list = [e["hash"] for e in audit_entries]
            placeholders = ",".join("?" * len(hash_list))
            cursor = db.execute(
                f"""SELECT id, hash, action, by_whom, note, timestamp
                    FROM adn_council_log
                    WHERE hash IN ({placeholders})
                    ORDER BY timestamp ASC""",
                hash_list
            )
            for row in cursor.fetchall():
                council_logs.append({
                    "id": row["id"],
                    "hash": row["hash"],
                    "action": row["action"],
                    "by_whom": row["by_whom"],
                    "note": row["note"],
                    "timestamp": row["timestamp"]
                })

        # Get ruling if it exists
        ruling_info = None
        ruling_cursor = db.execute(
            "SELECT ruling_id, ruling_text, decided_by, status, created_at FROM arbitration_rulings WHERE case_id = ?",
            (case_id,)
        )
        ruling_row = ruling_cursor.fetchone()

        if ruling_row:
            ruling_info = {
                "ruling_id": ruling_row["ruling_id"],
                "ruling_text": ruling_row["ruling_text"],
                "decided_by": ruling_row["decided_by"],
                "status": ruling_row["status"],
                "created_at": ruling_row["created_at"]
            }

        return {
            "case_info": case_info,
            "audit_entries": audit_entries,
            "council_logs": council_logs,
            "ruling_info": ruling_info,
            "total_entries": len(audit_entries)
        }

    except sqlite3.Error as e:
        logger.error(f"Database error: {e}")
        raise HTTPException(status_code=500, detail=f"Database error: {str(e)}")


@app.post("/audit/query", tags=["Audit"])
async def query_audit_trail(
    start_date: Optional[int] = Query(None, description="Unix timestamp (seconds) for start of range"),
    end_date: Optional[int] = Query(None, description="Unix timestamp (seconds) for end of range"),
    entity: Optional[str] = Query(None, description="Filter by sender or receiver entity name"),
    action: Optional[str] = Query(None, description="Filter by council action (commit/revoke)"),
    purpose: Optional[str] = Query(None, description="Filter by audit trail purpose (PROPOSE/ACCEPT/REFUSE/etc)"),
    limit: int = Query(100, ge=1, le=1000, description="Max results to return")
) -> Dict[str, Any]:
    """
    Query audit trail with flexible filtering

    Args:
        start_date: Unix timestamp for range start (optional)
        end_date: Unix timestamp for range end (optional)
        entity: Filter by sender or receiver (optional)
        action: Filter by council action type (optional)
        purpose: Filter by audit purpose (optional)
        limit: Maximum results to return (1-1000, default 100)

    Returns:
        results: List of filtered audit entries
        total_count: Total matching entries found
        query_params: Echo of query parameters used
    """
    try:
        db = get_db_connection()

        # Build dynamic query based on filters
        where_clauses = []
        params = []

        # Query audit_trail table first
        audit_query = "SELECT seq, hash, parent_hash, sender, receiver, purpose FROM audit_trail WHERE 1=1"

        if entity:
            audit_query += " AND (sender LIKE ? OR receiver LIKE ?)"
            params.extend([f"%{entity}%", f"%{entity}%"])

        if purpose:
            audit_query += " AND purpose = ?"
            params.append(purpose)

        if start_date:
            audit_query += " AND created_at >= ?"
            params.append(start_date)

        if end_date:
            audit_query += " AND created_at <= ?"
            params.append(end_date)

        audit_query += f" ORDER BY seq DESC LIMIT {limit}"

        audit_entries = []
        cursor = db.execute(audit_query, params)

        for row in cursor.fetchall():
            audit_entries.append({
                "seq": row["seq"],
                "hash": row["hash"],
                "parent_hash": row["parent_hash"],
                "sender": row["sender"],
                "receiver": row["receiver"],
                "purpose": row["purpose"]
            })

        # Query council logs if action filter is specified
        council_entries = []
        if action:
            council_query = "SELECT id, hash, action, by_whom, note, timestamp FROM adn_council_log WHERE action = ?"
            council_params = [action]

            if entity:
                council_query += " AND by_whom LIKE ?"
                council_params.append(f"%{entity}%")

            if start_date:
                council_query += " AND timestamp >= ?"
                council_params.append(start_date)

            if end_date:
                council_query += " AND timestamp <= ?"
                council_params.append(end_date)

            council_query += f" ORDER BY timestamp DESC LIMIT {limit}"

            council_cursor = db.execute(council_query, council_params)
            for row in council_cursor.fetchall():
                council_entries.append({
                    "id": row["id"],
                    "hash": row["hash"],
                    "action": row["action"],
                    "by_whom": row["by_whom"],
                    "note": row["note"],
                    "timestamp": row["timestamp"]
                })

        combined_results = audit_entries + council_entries

        return {
            "results": combined_results[:limit],
            "total_count": len(combined_results),
            "query_params": {
                "start_date": start_date,
                "end_date": end_date,
                "entity": entity,
                "action": action,
                "purpose": purpose,
                "limit": limit
            }
        }

    except sqlite3.Error as e:
        logger.error(f"Database error: {e}")
        raise HTTPException(status_code=500, detail=f"Database error: {str(e)}")


@app.get("/audit/stats", tags=["Audit"])
async def get_audit_stats() -> Dict[str, Any]:
    """
    Get overall statistics for audit trail

    Returns:
        total_entries: Total audit trail entries
        total_cases: Total arbitrage cases
        total_rulings: Total arbitration rulings
        council_actions: Count of council actions by type
        purposes: Count of audit entries by purpose
    """
    try:
        db = get_db_connection()

        # Get total counts
        total_entries = db.execute("SELECT COUNT(*) as count FROM audit_trail").fetchone()["count"]
        total_cases = db.execute("SELECT COUNT(*) as count FROM arbitrage_cases").fetchone()["count"]
        total_rulings = db.execute("SELECT COUNT(*) as count FROM arbitration_rulings").fetchone()["count"]

        # Get council actions breakdown
        council_actions = {}
        cursor = db.execute("SELECT action, COUNT(*) as count FROM adn_council_log GROUP BY action")
        for row in cursor.fetchall():
            council_actions[row["action"]] = row["count"]

        # Get purposes breakdown
        purposes = {}
        cursor = db.execute("SELECT purpose, COUNT(*) as count FROM audit_trail GROUP BY purpose")
        for row in cursor.fetchall():
            purposes[row["purpose"]] = row["count"]

        return {
            "total_entries": total_entries,
            "total_cases": total_cases,
            "total_rulings": total_rulings,
            "council_actions": council_actions,
            "purposes": purposes,
            "timestamp": datetime.utcnow().isoformat()
        }

    except sqlite3.Error as e:
        logger.error(f"Database error: {e}")
        raise HTTPException(status_code=500, detail=f"Database error: {str(e)}")


@app.get("/", tags=["Root"])
async def root() -> Dict[str, str]:
    """Root endpoint - API information"""
    return {
        "service": "CSTL Audit Trail Server",
        "version": "5.1.0",
        "endpoints": {
            "health": "GET /health",
            "audit_trail": "GET /audit/{case_id}",
            "query_audit": "POST /audit/query",
            "stats": "GET /audit/stats",
            "docs": "/docs"
        }
    }


async def run_server(host: str = "127.0.0.1", port: int = 8000):
    """Run FastAPI server"""
    config = uvicorn.Config(
        app,
        host=host,
        port=port,
        log_level="info"
    )
    server = uvicorn.Server(config)
    await server.serve()


if __name__ == "__main__":
    # Run with: python -m uvicorn cstl_adn_server:app --host 127.0.0.1 --port 8000
    # Or: python cstl_adn_server.py
    asyncio.run(run_server())
