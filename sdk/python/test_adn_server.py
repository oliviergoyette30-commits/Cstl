#!/usr/bin/env python3
"""
Simple test for CSTL ADN Server FastAPI endpoints
Tests basic connectivity and endpoint structure
"""

import sqlite3
import os
import tempfile
from pathlib import Path

def _create_test_db(db_path):
    """Base minimale (meme schema que les tables lues par cstl_adn_server)."""
    if True:
        conn = sqlite3.connect(db_path)

        # Create minimal schema
        conn.execute("""
            CREATE TABLE audit_trail (
                seq INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                parent_hash TEXT NOT NULL,
                sender TEXT NOT NULL,
                receiver TEXT NOT NULL,
                purpose TEXT NOT NULL
            )
        """)

        conn.execute("""
            CREATE TABLE arbitrage_cases (
                case_id TEXT PRIMARY KEY,
                initiator TEXT NOT NULL,
                subject TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                assigned_arbiters TEXT
            )
        """)

        conn.execute("""
            CREATE TABLE adn_council_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL,
                action TEXT NOT NULL,
                by_whom TEXT NOT NULL,
                note TEXT,
                timestamp INTEGER NOT NULL
            )
        """)

        conn.execute("""
            CREATE TABLE arbitration_rulings (
                ruling_id TEXT PRIMARY KEY,
                case_id TEXT NOT NULL UNIQUE,
                ruling_text TEXT NOT NULL,
                decided_by TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )
        """)

        # Insert test data
        import time
        now = int(time.time())

        # Insert audit trail entry
        conn.execute(
            "INSERT INTO audit_trail (hash, parent_hash, sender, receiver, purpose) VALUES (?, ?, ?, ?, ?)",
            ("sha256:abc123", "root", "agent_alice", "agent_bob", "PROPOSE")
        )

        # Insert arbitrage case
        conn.execute(
            "INSERT INTO arbitrage_cases (case_id, initiator, subject, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            ("case_001", "alice", "dispute_subject", "Open", now, now)
        )

        # Insert council log
        conn.execute(
            "INSERT INTO adn_council_log (hash, action, by_whom, note, timestamp) VALUES (?, ?, ?, ?, ?)",
            ("sha256:abc123", "commit", "human_reviewer", "Approved", now)
        )

        conn.commit()
        conn.close()


# Test database creation
def test_database():
    """Create and verify test database"""
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "test_adn.db")
        _create_test_db(db_path)
        conn = sqlite3.connect(db_path)
        assert conn.execute("SELECT COUNT(*) FROM audit_trail").fetchone()[0] == 1
        conn.close()

        print("✓ Test database created successfully")
        print(f"  Database path: {db_path}")
        print("  Tables created: audit_trail, arbitrage_cases, adn_council_log, arbitration_rulings")
        print("  Sample data inserted: 1 audit entry, 1 case, 1 council log entry")


def test_fastapi_imports():
    """Test FastAPI imports"""
    try:
        from fastapi import FastAPI, HTTPException, Query
        from fastapi.responses import JSONResponse
        print("✓ FastAPI imports successful")
    except ImportError as e:
        print(f"✗ FastAPI not available: {e}")
        print("  Install: pip install fastapi uvicorn")
        raise


def test_server_startup():
    """Test server can be initialized (without running)"""
    try:
        import sys
        sys.path.insert(0, os.path.dirname(__file__))

        import cstl_adn_server
        app = cstl_adn_server.app

        print("✓ FastAPI app initialized successfully")
        print(f"  App title: {app.title}")
        print(f"  App version: {app.version}")

        # Count routes
        routes = [route for route in app.routes]
        print(f"  Routes registered: {len(routes)}")
        for route in routes:
            if hasattr(route, 'path'):
                print(f"    - {route.path}")

    except ImportError as e:
        print(f"✗ Cannot import cstl_adn_server: {e}")
        raise



def _client_on_test_db(tmpdir):
    """TestClient sur une base de test ; le `with` declenche le lifespan."""
    import sys
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import cstl_adn_server
    from fastapi.testclient import TestClient
    db_path = os.path.join(tmpdir, "test_adn.db")
    _create_test_db(db_path)
    cstl_adn_server.conn = None
    cstl_adn_server.DB_PATH = db_path
    return cstl_adn_server, TestClient(cstl_adn_server.app)


def test_endpoints_against_real_db():
    """Endpoints reels via le lifespan FastAPI (ajoute le 2026-11-17 : aucun
    test n'appelait jamais un endpoint, ce qui a laisse /audit/stats
    inaccessible -- capture par /audit/{case_id})."""
    with tempfile.TemporaryDirectory() as tmpdir:
        server, client = _client_on_test_db(tmpdir)
        with client as c:
            assert server.conn is not None, "lifespan: connexion ouverte au demarrage"
            health = c.get("/health")
            assert health.status_code == 200
            assert health.json()["database"]["audit_trail_entries"] == 1

            stats = c.get("/audit/stats")
            assert stats.status_code == 200, stats.text
            assert stats.json()["total_entries"] == 1
            assert stats.json()["total_cases"] == 1

            case = c.get("/audit/case_001")
            assert case.status_code == 200, case.text

            missing = c.get("/audit/no_such_case")
            assert missing.status_code == 404
        assert server.conn is None, "lifespan: connexion fermee a l'arret"


def test_static_routes_declared_before_parametric_ones():
    """Regression : une route fixe /audit/xxx doit preceder /audit/{case_id}."""
    import sys
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import cstl_adn_server
    paths = [r.path for r in cstl_adn_server.app.routes if hasattr(r, "path")]
    param = paths.index("/audit/{case_id}")
    for p in paths:
        if p.startswith("/audit/") and "{" not in p and p != "/audit/query":
            assert paths.index(p) < param, f"{p} est masquee par /audit/{{case_id}}"

def main():
    print("🔍 CSTL ADN Server Testing Suite")
    print("=" * 50)

    print("\n1. Testing FastAPI imports...")
    test_fastapi_imports()

    print("\n2. Testing database schema...")
    test_database()

    print("\n3. Testing server initialization...")
    test_server_startup()

    print("\n" + "=" * 50)
    print("✅ All basic tests completed")
    print("\nTo run the full server:")
    print("  export PYTHONPATH=sdk/python:$PYTHONPATH")
    print("  pip install -r requirements.txt")
    print("  python -m uvicorn cstl_adn_server:app --host 127.0.0.1 --port 8000")


if __name__ == "__main__":
    main()
