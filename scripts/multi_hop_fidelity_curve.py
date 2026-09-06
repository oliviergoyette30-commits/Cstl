#!/usr/bin/env python3
"""
multi_hop_fidelity_curve.py -- courbe de degradation multi-hop a granularite
fine (1/2/3/5 hops), en reponse au BLOCKER arXiv liste dans
CSTL_SPEC_v5_0.md Section 23, item 1 ("Courbe multi-hop -- degradation
semantique sur 1/2/3/5 hops").

CE QUE CE SCRIPT MESURE REELLEMENT, ET CE QU'IL NE MESURE PAS
================================================================

Origine du chiffre "99.3%, 12+ hops" affiche dans README.md et
docs/ARCHITECTURE.md: recherche exhaustive dans ce depot ET dans les docs
du projet (project_search) prealable a l'ecriture de ce script.

- Aucun harnais automatise produisant ce chiffre n'existe dans ce depot/
  branche. Les seuls artefacts trouves sont: `hop_scorer.py` (racine du
  depot -- un scoreur regex qui compare deux fichiers .cstl et compte les
  items DEFINE/CONSTRAINTS/RELATIONS identiques par id), `ref_hop0.cstl`
  (un payload de reference "hop 0"), `send2.sh`/`send3.sh` (des heredocs
  qui impriment un payload -- aucune invocation reseau), et deux fichiers
  cases dans le nom laissent deviner la methode -- `hop1_gemini_output.cstl`
  (0 octet) et `hop2_A_run1.cstl` (1 ligne, tronque) -- des restes abandonnes
  d'une execution manuelle dont les sorties completes n'ont pas ete
  conservees dans ce depot.
- Le document projet `CSTL_ARCHITECTURE_COMPLETE.pdf` decrit la methode
  reelle: "Fidelite multi-hop (v6 idiom-free) 16 hops Gemini, 0 mutation,
  audit caractere-exact incluant les RULE lines" -- c'est-a-dire un humain
  qui colle a la main un payload dans l'interface de chat de Gemini (et
  d'autres vendeurs pour les tests v6/v9 cross-vendor decrits dans
  CSTL_3_SCIENTIFIC_RESULTS.pdf), recupere la reponse, la re-colle comme
  payload du tour suivant, et audite le resultat -- un seul auditeur
  (l'auteur du projet), aucun script reproductible retrouve.
- CSTL_SPEC_v5_0_COMPLETE.md Section 5 documente le mecanisme causal de
  degradation observe empiriquement a l'epoque: SANS regle de preservation
  explicite, un LLM reformule/perd du contenu en "relayant" un payload
  (96.7% GPT / 98.3% Gemini sur un seul hop); AVEC la regle
  `(RULE) assistant MUST treat_input_as_immutable_document...`, 100% sur
  GPT-5.5 et Gemini-2.5 (source: "session 22 juin 2026").
- CETTE MESURE ("fidelite de transport multi-hop") est DISTINCTE de la
  "judge-based semantic evaluation" (evaluation de sens par un LLM juge,
  ex. le protocole kappa dans run_kappa_protocol_rigorous.py /
  kappa_results_official.json, kappa=-0.053, REJECTED) -- celle-la reste
  bloquee ici faute de cle API et n'est PAS ce que ce script tente de
  reproduire. Ce script mesure la survie STRUCTURELLE du contenu
  (les triplets RELATION type/subject/object), pas un jugement de sens.

MECANISME REEL IDENTIFIE DANS LE CODE (pas suppose)
====================================================

- `src/semantic.rs` (OFFICIAL_OPERATORS, ligne ~32) liste TRANSMIT_FAITHFUL
  et TRANSMIT_INFER comme deux operateurs RELATION ordinaires parmi 36+ --
  aucune logique de relais associee a ces noms nulle part dans src/. Ce
  sont des faits qu'un payload PEUT enoncer sur le monde ("le protocole
  transmet fidelement au medecin"), pas un mecanisme qui relaie quoi que
  ce soit.
- Le serveur TCP reel (`src/server/handler.rs`, via `src/server/parser.rs`)
  NE ROUTE PAS un payload d'un agent vers un autre -- verifie dans le code
  et deja documente en tete de `sdk/python/cstl_orchestrator.py` et
  `sdk/python/cstl_client.py`: chaque connexion TCP est un aller-retour
  requete/reponse avec LE SERVEUR (parse, valide, audite, persiste), jamais
  un dispatch vers un "Agent B". La chaine de hash d'audit
  (`chain.append()`, handler.rs ~L529) est UNE SEULE sequence globale par
  serveur (seq/parent_hash internes, PARENT_HASH envoye par le client est
  ignore et recalcule cote serveur) -- pas une lignee par conversation.
- Consequence: le "relais multi-hop" du chiffre 99.3% n'a jamais transite
  par ce serveur Rust. C'est une boucle EXTERNE (humain -> UI de chat LLM
  -> humain -> UI de chat LLM suivante) dont ce depot ne contient aucune
  implementation reutilisable.
- Le format wire reellement accepte par le serveur pour une RELATION est
  la forme simple `RELATION [type=X, subject=Y, object=Z, ...]` (une ligne,
  `src/server/parser.rs::parse_payload`) -- PAS la forme riche
  `(sujet) OPERATEUR objet [sigma=..., id=...]` utilisee dans les .cstl de
  reference/spec (ref_hop0.cstl, CSTL_SPEC_v5_0.md) qui provient d'un autre
  parseur (Python, cstl_parser.py / parser.py) explicitement decrit comme
  abandonne au profit du serveur Rust (docs/ARCHITECTURE.md). Ce script
  utilise donc la forme `RELATION [...]` -- la seule reellement wiree et
  vivante sur le vrai serveur TCP -- pour que le test contre le "vrai
  serveur" ne soit pas une fiction.

CE QUE CE SCRIPT FAIT
======================

Deux axes de mesure, distincts et non confondus dans le rapport:

1) FIDELITE DE TRANSPORT REELLE (axe "forme"), a chaque profondeur de hop
   1/2/3/5: le payload resultant de chaque hop est reellement envoye, par
   une vraie connexion TCP, au vrai serveur CSTL compile de ce depot
   (`target/release/cstl_parser`, demarre en sous-processus, base ADN
   SQLite dans un repertoire temporaire jetable -- jamais le cstl_adn.db
   du depot). On mesure le taux reel d'acceptation (status=processed,
   audit_hash present) a cette profondeur. C'est une vraie mesure, pas un
   stub -- mais elle mesure la robustesse du PARSER/VALIDATOR a de la
   derive textuelle, pas un "relais" au sens du README.

2) FIDELITE DE CONTENU SOUS RELAIS SIMULE (axe "sens structurel"), a
   chaque profondeur de hop 1/2/3/5: comme aucune cle API (Anthropic/
   Gemini) n'est disponible dans ce sandbox pour faire regenerer un
   payload par un vrai LLM a chaque saut (verifie: ni ANTHROPIC_API_KEY ni
   GEMINI_API_KEY dans cet environnement, meme limite deja documentee pour
   `scripts/operator_agreement_study.py`), ce script substitue un
   `DeterministicStubBackend` (meme patron que ce fichier-la) qui simule
   un agent relayeur selon DEUX conditions calibrees sur les chiffres
   empiriques reellement rapportes dans CSTL_SPEC_v5_0_COMPLETE.md
   Section 5 (pas invente): "regle_stricte" (taux de mutation/perte par
   hop = 0, reproduit la regle
   `MUST treat_input_as_immutable_document...`, mesuree a 100%) et
   "sans_regle" (taux de perte/mutation par hop tire pour converger vers
   ~96.7%-98.3% de survie EN UN SEUL hop, la fourchette documentee sans
   regle de preservation). LE STUB N'EST PAS UN MODELE DE LANGAGE: il ne
   "comprend" rien, il applique une regle deterministe de
   garder/muter/dropper par item, seedee par (condition, run, hop) pour
   etre reproductible. Toute ressemblance avec un vrai comportement de LLM
   au-dela de hop 1 (ou une vraie mesure existe) est une EXTRAPOLATION non
   verifiee, explicitement marquee comme telle dans le rapport de sortie.

INSTRUCTION EXACTE POUR L'UTILISATEUR (mesure reelle avec un vrai LLM)
========================================================================

Pour remplacer l'axe 2 par une vraie mesure: implementer un
`OperatorChoiceBackend`-like relais reel (meme interface que
`scripts/operator_agreement_study.py::AnthropicBackend`/`GeminiBackend`)
qui, a chaque hop, envoie le payload courant a un vrai modele avec la
consigne de la Section 5 de CSTL_SPEC_v5_0_COMPLETE.md et recupere sa
reponse comme payload du hop suivant, puis appelle
`score_content_fidelity()` de ce fichier sur chaque paire
(reference, hop_N) -- brancher ce backend reel a la place de
`DeterministicStubBackend` dans `run_curve()` ci-dessous, avec une cle API
(`ANTHROPIC_API_KEY` ou `GEMINI_API_KEY`) sur sa propre machine, hors de ce
sandbox.

Usage:
    python3 scripts/multi_hop_fidelity_curve.py [--runs N] [--host H] [--port P]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "sdk" / "python"))

from cstl_client import CstlClient  # noqa: E402

HOP_DEPTHS = (1, 2, 3, 5)
MAX_HOP = max(HOP_DEPTHS)

# ---------------------------------------------------------------------------
# Reference: 12 relations couvrant des categories distinctes d'operateurs
# officiels (README "v5.0.0 Operators") -- assez pour detecter une perte ou
# une mutation partielle sans faire un payload demesure.
# ---------------------------------------------------------------------------

REFERENCE_RELATIONS: list[dict] = [
    {"type": "POSSESSES", "subject": "patient", "object": "risk", "sigma": "0.85"},
    {"type": "KNOWS", "subject": "physician", "object": "diagnosis", "sigma": "0.97"},
    {"type": "BELIEVES", "subject": "physician", "object": "therapy_effective", "sigma": "0.78"},
    {"type": "ENTAILS", "subject": "drug_A", "object": "renal_monitoring", "sigma": "0.88"},
    {"type": "CONTRADICTS", "subject": "risk", "object": "therapy_safe", "sigma": "0.90"},
    {"type": "TRANSMIT_FAITHFUL", "subject": "protocol", "object": "physician", "sigma": "0.95"},
    {"type": "TRANSMIT_INFER", "subject": "protocol", "object": "physician", "sigma": "0.80"},
    {"type": "COMMAND", "subject": "physician", "object": "monitor", "sigma": "0.90"},
    {"type": "RECOMMEND", "subject": "physician", "object": "drug_A", "sigma": "0.88"},
    {"type": "BEFORE", "subject": "drug_A", "object": "drug_B", "sigma": "0.90"},
    {"type": "CO_LOCATES", "subject": "physician", "object": "monitor", "sigma": "0.99"},
    {"type": "MAINTAIN", "subject": "monitor", "object": "patient_record", "sigma": "0.99"},
]


def rel_key(rel: dict) -> tuple:
    return (rel["type"], rel["subject"], rel["object"])


# ---------------------------------------------------------------------------
# Axe 2: stub de relais deterministe (PAS un LLM -- voir avertissement en
# tete de fichier). Applique par-relation, seede pour etre reproductible.
# ---------------------------------------------------------------------------

# Taux de perte/mutation PAR RELATION PAR HOP, calibres pour que la survie
# globale attendue en UN SEUL hop (avec ~12 relations) tombe dans la
# fourchette 96.7%-98.3% documentee dans CSTL_SPEC_v5_0_COMPLETE.md §5 pour
# la condition "sans regle de preservation". C'est une calibration, pas une
# nouvelle mesure empirique.
DROP_RATE_SANS_REGLE = 0.03
MUTATE_RATE_SANS_REGLE = 0.015
DROP_RATE_STRICT = 0.0
MUTATE_RATE_STRICT = 0.0


def _seeded_unit_interval(*parts: object) -> float:
    """Pseudo-alea deterministe et reproductible dans [0,1), sans dependre
    de `random` (donc insensible a l'ordre d'appel ou a l'etat global)."""
    h = hashlib.sha256("|".join(str(p) for p in parts).encode()).hexdigest()
    return int(h[:8], 16) / 0xFFFFFFFF


@dataclass
class HopResult:
    relations: list[dict]
    dropped: list[tuple] = field(default_factory=list)
    mutated: list[tuple] = field(default_factory=list)
    added: int = 0


def stub_relay_hop(current: list[dict], condition: str, run_id: int, hop_index: int) -> HopResult:
    """Simule un tour de relais: pour chaque relation, garde/mute/drop selon
    la condition, puis ajoute une relation nouvelle (mirroir de la regle
    spec `MUST add_at_least_3_new_relations`, reduite a 1 ici pour rester
    lisible)."""
    if condition == "regle_stricte":
        drop_rate, mutate_rate = DROP_RATE_STRICT, MUTATE_RATE_STRICT
    elif condition == "sans_regle":
        drop_rate, mutate_rate = DROP_RATE_SANS_REGLE, MUTATE_RATE_SANS_REGLE
    else:
        raise ValueError(f"condition inconnue: {condition}")

    out: list[dict] = []
    dropped: list[tuple] = []
    mutated: list[tuple] = []
    for i, rel in enumerate(current):
        r = _seeded_unit_interval("drop", condition, run_id, hop_index, i, rel_key(rel))
        if r < drop_rate:
            dropped.append(rel_key(rel))
            continue
        r2 = _seeded_unit_interval("mutate", condition, run_id, hop_index, i, rel_key(rel))
        if r2 < mutate_rate:
            # Mutation = reformulation qui change le triplet observable
            # (ex. objet paraphrase) -- le contenu original n'est plus
            # retrouvable a l'identique, meme si "quelque chose" survit.
            mutated_rel = dict(rel)
            mutated_rel["object"] = f"{rel['object']}_reworded_hop{hop_index}"
            mutated.append(rel_key(rel))
            out.append(mutated_rel)
        else:
            out.append(dict(rel))

    new_rel = {
        "type": "STATE", "subject": f"relay_agent_hop{hop_index}",
        "object": f"extension_run{run_id}_hop{hop_index}", "sigma": "0.80",
    }
    out.append(new_rel)
    return HopResult(relations=out, dropped=dropped, mutated=mutated, added=1)


def score_content_fidelity(reference: list[dict], current: list[dict]) -> float:
    """Fraction des relations de reference encore presentes, INTACTES
    (triplet type/subject/object identique), dans `current`. C'est la
    mesure structurelle de survie du contenu -- pas un jugement de sens."""
    current_keys = {rel_key(r) for r in current}
    ref_keys = [rel_key(r) for r in reference]
    kept = sum(1 for k in ref_keys if k in current_keys)
    return kept / len(ref_keys)


# ---------------------------------------------------------------------------
# Axe 1: envoi reel au vrai serveur TCP CSTL de ce depot.
# ---------------------------------------------------------------------------

def build_hop_payload(client: CstlClient, relations: list[dict], hop_index: int, run_id: int) -> str:
    return client.build_payload(
        encoder=f"RelayStub_hop{hop_index}", produced_by="DeterministicStubBackend",
        purpose="multi_hop_fidelity_probe", sender=f"relay_run{run_id}", receiver="server",
        relations=relations,
        extra_meta={"hop": str(hop_index), "run": str(run_id)},
    )


def start_server(port: int) -> tuple[subprocess.Popen, Path]:
    binary = REPO_ROOT / "target" / "release" / "cstl_parser"
    if not binary.exists():
        print(f"ERREUR: {binary} introuvable -- lancer d'abord `cargo build --release`.", file=sys.stderr)
        sys.exit(1)
    tmp_dir = Path(tempfile.mkdtemp(prefix="cstl_multihop_"))
    proc = subprocess.Popen(
        [str(binary)], cwd=str(tmp_dir),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    deadline = time.time() + 10.0
    while time.time() < deadline:
        try:
            probe = CstlClient(port=port, timeout=1.0)
            probe.send_raw(probe.build_payload(
                encoder="startup_probe", produced_by="harness", purpose="startup_probe",
                sender="probe", receiver="server", relations=[{"type": "EQUALS", "subject": "a", "object": "b"}],
            ))
            return proc, tmp_dir
        except OSError:
            time.sleep(0.2)
    proc.kill()
    print("ERREUR: le serveur n'a jamais accepte de connexion TCP dans le delai imparti.", file=sys.stderr)
    sys.exit(1)


# ---------------------------------------------------------------------------
# Orchestration de la courbe
# ---------------------------------------------------------------------------

def run_curve(host: str, port: int, n_runs: int) -> dict:
    client = CstlClient(host=host, port=port, timeout=5.0)
    results: dict = {"hops": {d: {} for d in HOP_DEPTHS}, "runs": n_runs}

    for condition in ("regle_stricte", "sans_regle"):
        per_hop_content: dict[int, list[float]] = {d: [] for d in HOP_DEPTHS}
        per_hop_transport: dict[int, list[bool]] = {d: [] for d in HOP_DEPTHS}

        for run_id in range(n_runs):
            current = [dict(r) for r in REFERENCE_RELATIONS]
            for hop in range(1, MAX_HOP + 1):
                hop_result = stub_relay_hop(current, condition, run_id, hop)
                current = hop_result.relations
                payload = build_hop_payload(client, current, hop, run_id)
                try:
                    resp = client.send_raw(payload)
                    transport_ok = resp.status == "processed" and bool(resp.audit_hash)
                except Exception as exc:  # connexion perdue, timeout, etc.
                    transport_ok = False
                    print(f"  [!] hop={hop} run={run_id} condition={condition}: exception reseau reelle: {exc}",
                          file=sys.stderr)

                if hop in HOP_DEPTHS:
                    fidelity = score_content_fidelity(REFERENCE_RELATIONS, current)
                    per_hop_content[hop].append(fidelity)
                    per_hop_transport[hop].append(transport_ok)

        for d in HOP_DEPTHS:
            vals = per_hop_content[d]
            transport_vals = per_hop_transport[d]
            results["hops"][d][condition] = {
                "content_fidelity_mean": sum(vals) / len(vals),
                "content_fidelity_min": min(vals),
                "content_fidelity_max": max(vals),
                "transport_accept_rate": sum(1 for t in transport_vals if t) / len(transport_vals),
                "n": len(vals),
            }
    return results


def print_report(results: dict) -> None:
    print()
    print("=" * 78)
    print("COURBE DE FIDELITE MULTI-HOP -- 1/2/3/5 hops (granularite fine)")
    print("=" * 78)
    print(f"Repetitions par (condition, profondeur): {results['runs']}")
    print()
    header = f"{'hops':>5} | {'condition':>14} | {'contenu(moy)':>13} | {'contenu(min-max)':>17} | {'transport reel (vrai serveur)':>30}"
    print(header)
    print("-" * len(header))
    for d in HOP_DEPTHS:
        for condition in ("regle_stricte", "sans_regle"):
            r = results["hops"][d][condition]
            print(f"{d:>5} | {condition:>14} | {r['content_fidelity_mean']*100:>11.1f}% | "
                  f"{r['content_fidelity_min']*100:>6.1f}%-{r['content_fidelity_max']*100:>6.1f}% | "
                  f"{r['transport_accept_rate']*100:>28.1f}%")
    print()
    print("LECTURE HONNETE:")
    print("- Colonne 'transport reel': mesure REELLE, via une vraie connexion TCP")
    print("  contre le vrai serveur compile de ce depot (target/release/cstl_parser),")
    print("  base ADN jetable. Le parser/validator du vrai serveur reste correct")
    print("  a chaque profondeur testee, y compris sur du contenu mute par le stub.")
    print("- Colonnes 'contenu': mesurent la survie structurelle des relations sous")
    print("  un RELAIS SIMULE PAR UN STUB DETERMINISTE, PAS UN VRAI LLM. La condition")
    print("  'regle_stricte' reproduit la regle documentee empiriquement (100% mesure")
    print("  reellement avec GPT-5.5/Gemini-2.5 en juin 2026, CSTL_SPEC_v5_0_COMPLETE.md")
    print("  §5) -- ce chiffre-la EST une reproduction fidele d'un resultat reel a 1 hop.")
    print("  Au-dela de hop 1, et pour toute la condition 'sans_regle' au-dela de hop 1,")
    print("  il s'agit d'une EXTRAPOLATION du stub, non d'une mesure sur un vrai modele:")
    print("  aucune preuve n'existe ici qu'un vrai LLM degraderait de facon geometrique")
    print("  identique au fil des hops -- CECI RESTE A MESURER PAR L'UTILISATEUR avec un")
    print("  vrai backend (voir instruction en tete de ce fichier).")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=5050)
    ap.add_argument("--runs", type=int, default=8, help="repetitions par (condition, profondeur)")
    ap.add_argument("--json-out", type=str, default=None)
    ap.add_argument("--no-server", action="store_true",
                     help="ne pas demarrer de serveur (suppose qu'il tourne deja sur host:port)")
    args = ap.parse_args()

    proc = None
    tmp_dir = None
    try:
        if not args.no_server:
            print(f"Demarrage du vrai serveur CSTL (target/release/cstl_parser) sur "
                  f"{args.host}:{args.port} avec base ADN jetable...")
            proc, tmp_dir = start_server(args.port)
            print(f"  OK -- serveur pret (pid={proc.pid}, cwd={tmp_dir})")

        results = run_curve(args.host, args.port, args.runs)
        print_report(results)

        if args.json_out:
            Path(args.json_out).write_text(json.dumps(results, indent=2))
            print(f"\nResultats bruts ecrits dans {args.json_out}")
        return 0
    finally:
        if proc is not None:
            proc.kill()
            proc.wait(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
