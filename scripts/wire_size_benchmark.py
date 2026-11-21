#!/usr/bin/env python3
"""
scripts/wire_size_benchmark.py -- taille de CSTL contre un JSON qui porte
EXACTEMENT TOUT (2026-11-21).

Pourquoi : l'ancien comparatif (`examples/benchmark_wire_formats.rs`)
etait injuste envers CSTL. Son JSON n'avait ni la version, ni le mode, ni
le bloc META (encoder, produced_by), et aucun message n'etait signe. Son
contenu etait synthetique (20 concepts « concept_i implies concept_i+1 »).

ICI :
  - les messages sont de VRAIS messages, captures sur le vrai serveur : un
    agent s'enregistre (cle publique, signature), le responsable pose une
    politique avec conditions, un agent envoie des faits, demande sa
    politique et une autorisation avec montant. On garde la requete ET la
    reponse du serveur (verification, coherence, gouvernance, audit, jeton
    signe) ;
  - chaque message CSTL est converti en JSON SANS RIEN PERDRE : version,
    mode, chaque bloc et chaque attribut, signatures comprises. La
    conversion est verifiee dans les deux sens (CSTL -> JSON -> blocs
    identiques) ; un message qui ne repasse pas est signale ;
  - deux JSON : compact (sans espaces, le plus petit possible) et indente
    (ce qu'ecrivent souvent les modeles et les journaux).

MESURES : octets bruts, octets apres gzip -9, tokens (API count_tokens de
Claude, gratuite) pour CSTL, JSON compact et JSON indente.

RESULTAT (2026-11-21, 16 vrais messages, 8 requetes et 8 reponses) :
                    octets   gzip -9   tokens (Claude)
  CSTL              13 046    6 511     6 625
  JSON compact      14 010    6 321     6 823
  JSON indente      17 691    6 639     8 414
- Contre le JSON compact, CSTL est 7 % plus petit en octets et 3 % en
  tokens, mais 3 % plus GROS apres gzip.
- Contre le JSON indente, CSTL est 26 % plus petit en octets et 21 % en
  tokens.
- Les signatures et les cles publiques (en hexadecimal) pesent autant dans
  les deux formats et ne se compressent pas.
- Le « 12 % » du banc format contre serveur venait d'un JSON avec des
  espaces apres « , » et « : » (json.dumps par defaut). Contre un JSON
  vraiment compact, l'ecart tombe a 3 %.

Usage :
  python3 scripts/wire_size_benchmark.py                 # octets seulement
  ANTHROPIC_API_KEY=... python3 scripts/wire_size_benchmark.py --tokens
"""
import argparse
import gzip
import json
import os
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.path.insert(0, str(REPO / "sdk" / "python"))
import team_task_benchmark as tb  # noqa: E402
from cstl_client import CstlClient, _parse_kv_block  # noqa: E402

HEADER = re.compile(r"^#!CSTL (\S+) MODE=(\S+)\s*$")
BLOCK = re.compile(r"^([A-Z_]+) \[(.*)\]\s*$")


def blocks_of(wire):
    """(version, mode, [(nom, {attributs}) ...], lignes non reconnues)."""
    version = mode = None
    blocks, other = [], []
    for line in wire.strip().splitlines():
        if line.strip() in ("", "---END---"):
            continue
        h = HEADER.match(line)
        if h:
            version, mode = h.groups()
            continue
        b = BLOCK.match(line)
        if b:
            blocks.append((b.group(1), _parse_kv_block(b.group(2))))
        else:
            other.append(line)
    return version, mode, blocks, other


def to_json(wire):
    """JSON idiomatique et complet : un objet par bloc unique, une liste pour
    les blocs repetes, dans l'ordre d'apparition."""
    version, mode, blocks, other = blocks_of(wire)
    names = [n for n, _ in blocks]
    obj = {"cstl": version, "mode": mode}
    for n, kv in blocks:
        key = n.lower()
        if names.count(n) > 1:
            obj.setdefault(key, []).append(kv)
        else:
            obj[key] = kv
    if other:
        obj["lines"] = other
    return obj


def from_json(obj):
    """Blocs reconstruits a partir du JSON, pour verifier qu'il ne manque rien."""
    out = []
    for k, v in obj.items():
        if k in ("cstl", "mode", "lines"):
            continue
        for kv in v if isinstance(v, list) else [v]:
            out.append((k.upper(), kv))
    return obj["cstl"], obj["mode"], out, obj.get("lines", [])


def same(wire, obj):
    v1, m1, b1, o1 = blocks_of(wire)
    v2, m2, b2, o2 = from_json(obj)
    return (v1, m1, o1) == (v2, m2, o2) and sorted(map(repr, b1)) == sorted(map(repr, b2))


def capture():
    """Joue un vrai scenario sur le vrai serveur et garde chaque echange."""
    log = []
    orig = CstlClient.send_raw

    def spy(self, payload):
        resp = orig(self, payload)
        log.append((payload, resp.raw))
        return resp

    CstlClient.send_raw = spy
    proc = tb.start_server("owner_*")
    try:
        owner = tb.new_agent(None, "owner_size")
        ex = tb.new_agent(None, "exec_size")
        base = {"type": "PERFORM", "subject": "exec_size"}
        owner.send_relations("server", [
            {**base, "object": "refund_order", "modality": "MAY", "amount_at_most": "100", "total_at_most": "1500",
             "reason": "remboursements dans le plafond et dans le budget"},
            {**base, "object": "refund_order", "modality": "MUST_NOT", "amount_above": "100", "reason": "au-dela du plafond"},
            {**base, "object": "share_medical_record", "modality": "MUST_NOT", "unless_fact": "patient_consent",
             "reason": "sans consentement du patient"},
            {**base, "object": "log_transaction", "modality": "MUST", "reason": "tracabilite"},
        ])
        ex.send_relations("server", [
            {"type": "STATE", "subject": f"order_{i}", "object": s}
            for i, s in enumerate(["paid", "shipped", "delivered", "returned", "refunded"])])
        ex.policy()
        ex.request_action("refund_order", 80)
        ex.request_action("share_medical_record", None)
        ex.send_relations("server", [
            {"type": "CAUSES", "subject": f"step_{i}", "object": f"step_{i + 1}", "confidence": "0.9"} for i in range(20)])
    finally:
        proc.terminate()
        proc.wait(timeout=10)
        CstlClient.send_raw = orig
    names = ["enregistrement (owner)", "enregistrement (exec)", "politique, 4 regles avec conditions",
             "5 faits", "demande de politique", "autorisation accordee (jeton signe)", "autorisation refusee",
             "20 relations causales"]
    msgs = []
    for name, (req, resp) in zip(names, log):
        msgs.append((f"{name} : requete", req))
        msgs.append((f"{name} : reponse", resp))
    return msgs


def gz(b):
    return len(gzip.compress(b, compresslevel=9, mtime=0))


def counter(enabled):
    if not enabled:
        return None
    import anthropic
    client = anthropic.Anthropic(api_key=os.environ["ANTHROPIC_API_KEY"], base_url="https://api.anthropic.com")

    def count(text):
        # count_tokens compte aussi l'enveloppe du message : on la retire
        n = client.messages.count_tokens(model="claude-opus-5-5", messages=[{"role": "user", "content": text}]).input_tokens
        return n - EMPTY[0]
    EMPTY = [0]
    EMPTY[0] = client.messages.count_tokens(model="claude-opus-5-5", messages=[{"role": "user", "content": "x"}]).input_tokens - 1
    return count


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tokens", action="store_true", help="compter aussi les tokens (cle API requise)")
    args = ap.parse_args()
    count = counter(args.tokens)
    rows, broken = [], []
    for name, wire in capture():
        obj = to_json(wire)
        if not same(wire, obj):
            broken.append(name)
        c = wire.encode()
        jc = json.dumps(obj, ensure_ascii=False, separators=(",", ":")).encode()
        ji = json.dumps(obj, ensure_ascii=False, indent=2).encode()
        r = {"message": name, "cstl": len(c), "json": len(jc), "json_indente": len(ji),
             "cstl_gz": gz(c), "json_gz": gz(jc), "json_indente_gz": gz(ji),
             "unparsed_lines": len(blocks_of(wire)[3])}
        if count:
            r.update({"cstl_tok": count(c.decode()), "json_tok": count(jc.decode()), "json_indente_tok": count(ji.decode())})
        rows.append(r)
    tot = {k: sum(r[k] for r in rows) for k in rows[0] if k != "message"}

    def pct(a, b):
        return f"{100 * (a - b) / b:+.0f}%"

    print(f"{'message':52} | {'CSTL':>6} {'JSON':>6} {'indente':>7} | gzip : {'CSTL':>5} {'JSON':>5}"
          + (f" | tokens : {'CSTL':>5} {'JSON':>5} {'indente':>7}" if count else ""))
    for r in rows + [{"message": "TOTAL", **tot}]:
        line = (f"{r['message'][:52]:52} | {r['cstl']:>6} {r['json']:>6} {r['json_indente']:>7} | "
                f"       {r['cstl_gz']:>5} {r['json_gz']:>5}")
        if count:
            line += f" |          {r['cstl_tok']:>5} {r['json_tok']:>5} {r['json_indente_tok']:>7}"
        print(line)
    print(f"\nCSTL par rapport au JSON compact : octets {pct(tot['cstl'], tot['json'])}, gzip {pct(tot['cstl_gz'], tot['json_gz'])}"
          + (f", tokens {pct(tot['cstl_tok'], tot['json_tok'])}" if count else ""))
    print(f"CSTL par rapport au JSON indente : octets {pct(tot['cstl'], tot['json_indente'])}, gzip {pct(tot['cstl_gz'], tot['json_indente_gz'])}"
          + (f", tokens {pct(tot['cstl_tok'], tot['json_indente_tok'])}" if count else ""))
    print(f"Conversion verifiee dans les deux sens : {len(rows) - len(broken)}/{len(rows)} messages"
          + (f" (ECHEC : {broken})" if broken else "") + f" ; lignes non reconnues : {tot['unparsed_lines']}")
    dest = REPO / "scripts" / "wire_size_benchmark.json"
    dest.write_text(json.dumps({"total": tot, "rows": rows, "broken": broken}, indent=2, ensure_ascii=False) + "\n")
    print(f"Resultats bruts : {dest.relative_to(REPO)}")


if __name__ == "__main__":
    main()
