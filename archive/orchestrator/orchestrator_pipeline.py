#!/usr/bin/env python3
"""CSTL orchestrator pipeline.

Step 1 (implemented): identity resolution. Sends a CSTL payload through an
OpenClaw agent turn, then replaces ORCHESTRATOR_PENDING in the response's
encoder= and produced_by= fields with the model that actually generated it.

The model must never fill in these fields itself (COPIED_RULES forbids it) —
this script is the orchestrator layer that has ground truth about which
provider/model handled the call, taken from the agent run's own metadata
rather than from anything the model claims about itself.

Step 2 (implemented): for every relation tagged
verified=confirmed_external_source, call cstl_verify_public_kb.verify_relation()
(existing script, not reimplemented here) and downgrade the status to
unchallenged_unproven if Wikidata doesn't actually confirm it. As of
2026-08-12, verify_relation() can confirm a fact via a multi-hop transitive
chain (e.g. P131 tour -> arrondissement -> ville) instead of only a direct
1-hop link; when it does, every hop in that chain is recorded as a
permanent dependency in hash_entanglement (cstl_hash_entanglement.py,
existing script, not reimplemented here) via record_dependency(), so that
if an intermediate fact is ever invalidated, find_dependents() can trace
every CSTL relation that was resting on it.
"""

import argparse
import hashlib
import importlib.util
import json
import re
import subprocess
import sys

PENDING = "ORCHESTRATOR_PENDING"

VERIFY_KB_PATH = "/data/data/com.termux/files/home/cstl/orchestrator/cstl_verify_public_kb.py"
HASH_ENTANGLEMENT_PATH = "/data/data/com.termux/files/home/cstl_hash_entanglement.py"
ADN_DB_PATH = "/data/data/com.termux/files/home/cstl_adn.db"

CONVERSATION_ID_RE = re.compile(r'CONVERSATION_ID=([^,\]\n]+)')

# (subject) VERB object [attrs]
RELATION_RE = re.compile(
    r'^\((?P<subject>[A-Za-z0-9_]+)\)\s+(?P<verb>[A-Z_]+)\s+(?P<object>[A-Za-z0-9_]+)\s*'
    r'\[(?P<attrs>[^\]]*)\]',
    re.MULTILINE,
)

CONFIRMED_STATUS = "verified=confirmed_external_source"
DOWNGRADED_STATUS = "verified=unchallenged_unproven"

# Matches lines like "encoder=ORCHESTRATOR_PENDING," or "produced_by=ORCHESTRATOR_PENDING"
# inside the META [ ... ] block, preserving indentation and trailing comma.
IDENTITY_FIELD_RE = re.compile(
    r'^(?P<indent>[ \t]*)(?P<field>encoder|produced_by)=(?P<value>' + PENDING + r')(?P<trail>,?)[ \t]*$',
    re.MULTILINE,
)


class OrchestratorError(RuntimeError):
    pass


def run_agent_turn(payload_path, agent="main", session_key=None, model=None, timeout=180):
    """Run the payload through `openclaw agent` and return the parsed JSON result."""
    cmd = [
        "openclaw", "agent",
        "--agent", agent,
        "--message-file", payload_path,
        "--json",
        "--timeout", str(timeout),
    ]
    if session_key:
        cmd += ["--session-key", session_key]
    if model:
        cmd += ["--model", model]

    # openclaw's own --timeout can be exceeded internally by a gateway-timeout
    # -> embedded-fallback retry (observed: ~150s gateway wait + ~60s embedded
    # run), so give the subprocess a generous margin on top of it.
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout + 150)
    if proc.returncode != 0:
        raise OrchestratorError(f"openclaw agent exited {proc.returncode}: {proc.stderr.strip()}")

    # openclaw writes log lines to stdout before the JSON result; the result
    # is the final top-level JSON object, so take everything from the last
    # line that starts a top-level '{'.
    stdout = proc.stdout
    start = stdout.rfind("\n{")
    json_text = stdout[start + 1:] if start != -1 else stdout
    try:
        parsed = json.loads(json_text)
    except json.JSONDecodeError as exc:
        raise OrchestratorError(f"could not parse JSON from openclaw agent output: {exc}") from exc

    # Without --deliver, the CLI wraps the turn result under "result"
    # ({runId, status, summary, result: {payloads, meta}}). With --deliver
    # it returns that inner shape directly. Normalize to the inner shape.
    return parsed.get("result", parsed)


def extract_response_text(result):
    text = result.get("finalAssistantVisibleText")
    if text:
        return text
    payloads = result.get("payloads") or []
    if payloads and payloads[0].get("text"):
        return payloads[0]["text"]
    raise OrchestratorError("no response text found in agent result JSON")


def extract_model_identity(result):
    agent_meta = (result.get("meta") or {}).get("agentMeta") or {}
    provider = agent_meta.get("provider")
    model = agent_meta.get("model")
    if not provider or not model:
        raise OrchestratorError("agentMeta.provider/model not found in agent result JSON")
    return f"{provider}/{model}"


def resolve_identity(document_text, identity):
    """Replace ORCHESTRATOR_PENDING in encoder=/produced_by= with `identity`.

    Returns (resolved_text, replaced_count). Only touches encoder=/produced_by=
    lines whose value is exactly ORCHESTRATOR_PENDING — every other line in the
    document is left untouched, per the immutability rule.
    """
    def _sub(m):
        return f"{m.group('indent')}{m.group('field')}={identity}{m.group('trail')}"

    resolved, count = IDENTITY_FIELD_RE.subn(_sub, document_text)
    return resolved, count


def load_verify_relation(path=VERIFY_KB_PATH):
    """Load verify_relation() from the existing, already-tested
    cstl_verify_public_kb.py without copying or reimplementing it."""
    spec = importlib.util.spec_from_file_location("cstl_verify_public_kb", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.verify_relation


def load_hash_entanglement_cls(path=HASH_ENTANGLEMENT_PATH):
    """Load HashEntanglement from the existing, already-tested
    cstl_hash_entanglement.py without copying or reimplementing it."""
    spec = importlib.util.spec_from_file_location("cstl_hash_entanglement", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.HashEntanglement


def _sha256_hash(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def _wikidata_fact_hash(from_qid, property_id, to_qid):
    """Canonical, reproducible hash for one Wikidata edge, so the same real
    -world fact always hashes the same way regardless of which CSTL payload
    or session referenced it."""
    return _sha256_hash(f"wikidata_fact:{from_qid}:{property_id}:{to_qid}")


def _extract_conversation_id(document_text):
    m = CONVERSATION_ID_RE.search(document_text)
    return m.group(1).strip() if m else None


def _humanize(token):
    """Entity id token -> search label. 'eiffel_tower' -> 'Eiffel Tower'."""
    return token.replace("_", " ").strip().title()


def _parse_attrs(attrs_text):
    """Split top-level comma-separated key=value attrs.

    Assumes attribute values (notably note=) contain no literal commas —
    true for every CSTL payload seen so far. A fully general CSTL parser
    would need quoting/escaping to lift this assumption.
    """
    attrs = {}
    for part in attrs_text.split(","):
        part = part.strip()
        if "=" not in part:
            continue
        key, _, value = part.partition("=")
        attrs[key.strip()] = value.strip()
    return attrs


def verify_relations(document_text, verify_fn=None, verbose=True,
                      entangle=True, entanglement=None, adn_db_path=ADN_DB_PATH):
    """Step 2: verify every relation tagged verified=confirmed_external_source
    against Wikidata via cstl_verify_public_kb.verify_relation().

    Confirmed relations, and every other line/field in the document, are left
    untouched. Unconfirmed ones (entity not resolved, SPARQL query failed, or
    the specific fact just isn't in Wikidata) get their status downgraded to
    unchallenged_unproven — nothing else in that relation changes.

    When a relation is confirmed via a multi-hop transitive chain (verify_fn
    returns a non-empty "chain"), every hop is recorded as a permanent
    dependency in hash_entanglement via record_dependency(): source_hash is a
    canonical hash of that one Wikidata edge, dependent_hash is a hash of
    this exact relation as it appears in this document. That's what lets
    find_dependents() later trace every CSTL relation resting on a given
    intermediate fact, should that fact ever be invalidated. Pass
    entangle=False to skip this (e.g. for dry runs / tests with a fake
    verify_fn that never returns a chain).

    Returns (resolved_text, report); report is a list of per-relation dicts
    with the raw Wikidata result plus any entanglement hops recorded.
    """
    if verify_fn is None:
        verify_fn = load_verify_relation()

    if entangle and entanglement is None:
        entanglement = load_hash_entanglement_cls()(adn_db_path)

    conversation_id = _extract_conversation_id(document_text)

    edits = []
    report = []

    for m in RELATION_RE.finditer(document_text):
        attrs_text = m.group("attrs")
        attrs = _parse_attrs(attrs_text)
        if attrs.get("verified") != "confirmed_external_source":
            continue

        subject_label = _humanize(m.group("subject"))
        object_label = _humanize(m.group("object"))
        predicate = attrs.get("predicate", "")
        relation_id = attrs.get("id", "?")

        if verbose:
            print(
                f"[verify_relations] {relation_id}: "
                f"({subject_label}) {predicate or m.group('verb')} ({object_label})",
                file=sys.stderr,
            )

        result = verify_fn(subject_label, predicate, object_label)
        confirmed = result.get("verified") == "confirmed_external_source"
        chain = result.get("chain")

        entangled_hops = []
        if confirmed and chain and entangle:
            relation_hash = _sha256_hash(m.group(0))
            property_id = result.get("property_id")
            for from_qid, to_qid in chain:
                source_hash = _wikidata_fact_hash(from_qid, property_id, to_qid)
                entanglement.record_dependency(
                    source_hash=source_hash,
                    dependent_hash=relation_hash,
                    dependent_context=conversation_id,
                    dependent_conversation_id=conversation_id,
                    note=f"hop {from_qid}->{to_qid} via {property_id} in chain "
                         f"supporting relation {relation_id} "
                         f"({subject_label} {predicate} {object_label})",
                )
                if verbose:
                    print(
                        f"[verify_relations]   entangled hop: {from_qid} -{property_id}-> "
                        f"{to_qid}  (source_hash={source_hash})",
                        file=sys.stderr,
                    )
                entangled_hops.append({
                    "from_qid": from_qid, "to_qid": to_qid,
                    "property": property_id, "source_hash": source_hash,
                })

        report.append({
            "id": relation_id,
            "subject": subject_label,
            "predicate": predicate,
            "object": object_label,
            "wikidata_result": result,
            "downgraded": not confirmed,
            "entangled_hops": entangled_hops,
        })

        if not confirmed:
            attrs_start = m.start("attrs")
            local_idx = attrs_text.find(CONFIRMED_STATUS)
            target_start = attrs_start + local_idx
            target_end = target_start + len(CONFIRMED_STATUS)
            edits.append((target_start, target_end, DOWNGRADED_STATUS))

    resolved = document_text
    for start, end, replacement in sorted(edits, key=lambda e: e[0], reverse=True):
        resolved = resolved[:start] + replacement + resolved[end:]

    return resolved, report


def main():
    parser = argparse.ArgumentParser(description="CSTL orchestrator — step 1: identity resolution")
    parser.add_argument("payload", help="path to the CSTL payload file")
    parser.add_argument("--agent", default="main")
    parser.add_argument("--session-key")
    parser.add_argument("--model", help="model override for this turn (provider/model)")
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--skip-verify", action="store_true",
                         help="skip step 2 (Wikidata verification)")
    parser.add_argument("-o", "--output", help="write resolved document here (default: stdout)")
    parser.add_argument("--report", help="write step-2 verification report (JSON) here")
    args = parser.parse_args()

    result = run_agent_turn(
        args.payload,
        agent=args.agent,
        session_key=args.session_key,
        model=args.model,
        timeout=args.timeout,
    )

    response_text = extract_response_text(result)
    identity = extract_model_identity(result)
    resolved_text, replaced = resolve_identity(response_text, identity)

    print(f"[orchestrator] step 1: resolved identity: {identity}", file=sys.stderr)
    print(f"[orchestrator] step 1: ORCHESTRATOR_PENDING fields replaced: {replaced}", file=sys.stderr)
    if replaced == 0:
        print("[orchestrator] WARNING: no ORCHESTRATOR_PENDING fields found — "
              "the model may have self-assigned encoder=/produced_by=, "
              "which COPIED_RULES forbids.", file=sys.stderr)

    verify_report = []
    if not args.skip_verify:
        resolved_text, verify_report = verify_relations(resolved_text)
        downgraded = sum(1 for r in verify_report if r["downgraded"])
        print(f"[orchestrator] step 2: relations checked: {len(verify_report)}, "
              f"downgraded: {downgraded}", file=sys.stderr)

    if args.output:
        with open(args.output, "w") as f:
            f.write(resolved_text)
        print(f"[orchestrator] wrote resolved document to {args.output}", file=sys.stderr)
    else:
        print(resolved_text)

    if args.report and verify_report:
        with open(args.report, "w") as f:
            json.dump(verify_report, f, indent=2, ensure_ascii=False)
        print(f"[orchestrator] wrote verification report to {args.report}", file=sys.stderr)


if __name__ == "__main__":
    main()
