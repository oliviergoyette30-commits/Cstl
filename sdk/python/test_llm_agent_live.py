#!/usr/bin/env python3
"""
test_llm_agent_live.py -- CstlAgent contre le VRAI serveur Rust (2026-11-17).

Pourquoi : avant ce test, l'agent LLM n'avait jamais ete execute contre le
serveur. Verifie en direct le 2026-11-17 : enregistrement "rejected",
messages "parse_error", et pourtant {"status": "sent"} rapporte a l'appelant.
Toutes les verifications logiques de CSTL ne s'appliquaient donc a AUCUN
message d'agent LLM.

Le modele est remplace par un stub deterministe (aucune cle API, aucun Ollama
requis) : ce test couvre le chemin agent -> serveur, pas la qualite du modele.
Saute si target/debug/cstl_parser n'est pas compile.
"""
import os
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import pytest

SDK = Path(__file__).resolve().parent
REPO = SDK.parent.parent
BINARY = REPO / "target" / "debug" / "cstl_parser"
sys.path.insert(0, str(SDK))


@pytest.fixture(scope="module")
def server():
    if not BINARY.exists():
        pytest.skip("cargo build --bin cstl_parser requis")
    # Port fixe (src/main.rs). Un serveur deja present -- par ex. un orphelin
    # d'une session precedente -- ferait tourner ce test contre un AUTRE etat
    # (vu en vrai : rotation_proof_required sur un nom deja enregistre).
    try:
        socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
        pytest.fail("le port 5050 est deja occupe : arreter l'autre serveur CSTL d'abord")
    except OSError:
        pass
    workdir = tempfile.mkdtemp(prefix="cstl_agent_live_")
    vault = Path(workdir) / "vault"
    vault.mkdir()
    env = {**os.environ, "OBSIDIAN_VAULT_PATH": str(vault)}
    proc = subprocess.Popen([str(BINARY)], cwd=workdir, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(120):
        try:
            socket.create_connection(("127.0.0.1", 5050), timeout=0.5).close()
            socket.create_connection(("127.0.0.1", 8000), timeout=0.5).close()
            break
        except OSError:
            time.sleep(0.25)
    else:
        proc.terminate()
        pytest.fail("serveur injoignable")
    yield Path(workdir)
    proc.terminate()
    proc.wait(timeout=10)


def make_agent(monkeypatch, tmp_path, answers):
    import cstl_llm_agent as m
    it = iter(answers)

    class Stub(m.LLMProvider):
        def generate(self, prompt, max_tokens=500):
            return next(it)

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(m.HermesAgentBrain, "from_env", classmethod(lambda cls: Stub()))
    return m, m.CstlAgent(f"agent_{uuid.uuid4().hex[:12]}", "hermes")


def test_agent_registers_sends_verifiable_relations_and_reads_flags(server, monkeypatch, tmp_path):
    m, agent = make_agent(monkeypatch, tmp_path, [
        'Voici: {"type": "PERFORM", "subject": "bot_t", "object": "wipe_db_t", "modality": "MUST_NOT"} ok',
        '{"type": "PERFORM", "subject": "bot_t", "object": "wipe_db_t", "modality": "MUST"}',
        '{"type": "TERMINATES_IF", "subject": "contract", "object": "breach"}',
        "desole, je ne sais pas",
    ])
    assert agent.register(), "l'enregistrement signe doit etre accepte"

    rel, resp = agent.tell("bob", "sujet")
    assert rel["modality"] == "MUST_NOT"
    assert not resp.is_error and resp.issues == [], m.describe(resp)

    # Contradiction deontique avec l'historique : acceptee mais SIGNALEE.
    rel, resp = agent.tell("bob", "sujet")
    assert not resp.is_error
    assert any(i.startswith("DEONTIC_AUDIT") for i in resp.issues), m.describe(resp)

    # Operateur hors catalogue : REJETE, raison visible.
    _, resp = agent.tell("bob", "sujet")
    assert resp.is_error and "E101" in m.describe(resp)

    # Reponse du modele inexploitable : rien n'est envoye.
    assert agent.tell("bob", "sujet") == (None, None)


def test_free_text_with_commas_is_signed_and_accepted(server, monkeypatch, tmp_path):
    m, agent = make_agent(monkeypatch, tmp_path, [])
    assert agent.register()
    resp = agent.send_message("bob", "Bonjour bob, la commande 42 est partie, merci [urgent]")
    assert not resp.is_error, m.describe(resp)


def test_human_sees_agent_contradiction_in_obsidian_and_graphify(server, monkeypatch, tmp_path):
    """Chaine complete : un agent LLM se contredit (MUST apres MUST_NOT sur la
    meme action) -> le serveur le signale -> l'humain le voit dans son vault
    Obsidian ET dans le graphe Graphify (2026-11-17 : avant, ni l'un ni
    l'autre ne montraient une contradiction deontique)."""
    import json
    import urllib.request
    m, agent = make_agent(monkeypatch, tmp_path, [
        '{"type": "PERFORM", "subject": "robot_g", "object": "purge_logs", "modality": "MUST_NOT"}',
        '{"type": "PERFORM", "subject": "robot_g", "object": "purge_logs", "modality": "MUST"}',
    ])
    assert agent.register()
    agent.tell("bob", "logs")
    _, resp = agent.tell("bob", "logs")
    assert any(i.startswith("DEONTIC_AUDIT") for i in resp.issues), m.describe(resp)

    note = (server / "vault" / "CSTL_Restricted_Council.md").read_text()
    assert "robot_g purge_logs: MUST vs MUST_NOT" in note, note[-500:]

    with urllib.request.urlopen("http://127.0.0.1:8000/graphify/export", timeout=5) as r:
        graph = json.load(r)
    conflicts = [e for e in graph["edges"] if e["type"] == "relation"
                 and e["source"] == "entity:robot_g" and e["metadata"].get("conflict") == "deontic"]
    assert len(conflicts) == 2, [e for e in graph["edges"] if e["source"] == "entity:robot_g"]

    # Provenance cryptographique (2026-11-20) : les messages de cet agent sont
    # revérifies a l'export, relies a SA cle, et chaines a leur predecesseur.
    mine = [n for n in graph["nodes"] if n["type"] == "audit_entry"
            and n["metadata"].get("sender") == agent.name]
    assert mine and all(n["metadata"].get("signature") == "valid" for n in mine), mine
    assert all(n["metadata"].get("key_matches_sender") == "true" for n in mine), mine
    assert all(n["metadata"].get("chain_ok") == "true" for n in mine), mine
    signed = {e["source"] for e in graph["edges"] if e["type"] == "signed_by"}
    assert {n["id"] for n in mine} <= signed

