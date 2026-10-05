#!/usr/bin/env python3
# CSTL — PIPELINE KAPPA v2, PROTECTION DETERMINISTE DES LITTERAUX
# (2026-10-05, reponse directe a l'objection d'Olivier sur kappa_generate_real.py:
#  "on sait deja que les prompts sont pas fiables" -- corriger le prompt d'encodage
#  pour qu'il "fasse attention" aux nombres/termes techniques reviendrait a faire
#  confiance a une instruction LLM, exactement ce que ce projet refuse de faire
#  partout ailleurs (PARENT_HASH calcule par un orchestrateur externe, pas genere
#  par texte; identite geree hors-LLM; validation R1-R13 deterministe en Rust).
#
# CE QUE CE SCRIPT CHANGE PAR RAPPORT A kappa_generate_real.py:
#   Avant: ENCODE et RECONSTRUCT sont deux appels LLM qui manipulent directement
#   le texte, y compris les valeurs numeriques/techniques -- rien n'empeche une
#   paraphrase qui perd la precision (c'est ce qui a ete mesure sur complex_002
#   "160 mmHg" et complex_003 "Bayesian optimization Thompson sampling" dans le
#   run reel du 2026-09-22, kappa=0.6296).
#
#   Maintenant: un ORCHESTRATEUR DETERMINISTE (ce script, aucun appel LLM pour
#   cette partie) remplace les litteraux PAR DES TOKENS OPAQUES avant que le LLM
#   ne voie le texte -- reutilise le mecanisme DEJA mesure et prouve resistant a
#   la mutation (tokens alphanumeriques opaques du type AGT7X_K3, voir les
#   trouvailles de session sur EXTENSION_ERROR_RATE/IMMUTABILITY_ERROR_RATE). Le
#   LLM ne voit jamais la vraie valeur -- il ne peut donc pas la paraphraser, pas
#   besoin de lui faire confiance pour ca. L'orchestrateur restaure les vraies
#   valeurs APRES coup, mecaniquement, avant de montrer quoi que ce soit aux
#   juges.
#
# CE QUE CA NE REGLE PAS (a dire clairement, pas a cacher) : edge_001 ("Alice
# does not believe Bob left early") n'a AUCUN litteral numerique/technique a
# proteger -- c'est un cas de negation epistemique, un probleme de structure
# logique, pas de precision litterale. Cette redesign ne le touchera pas. Si le
# desaccord des juges persiste dessus, c'est un vrai signal sur CSTL, pas un
# artefact de methodologie a corriger.
#
# Etape 1 (ce script, Termux, ANTHROPIC_API_KEY requise):
#   pour chacun des 20 textes:
#     (a) REDACT (deterministe, orchestrateur): remplace les litteraux proteges
#         par des tokens opaques dans le texte source
#     (b) ENCODE (LLM): represente le texte REDIGE en CSTL -- ne voit jamais les
#         vraies valeurs
#     (c) RESTORE_CSTL (deterministe): verifie que chaque token attendu est
#         present dans le CSTL genere -- log LITERAL_LOST si absent, AVANT meme
#         d'aller plus loin
#     (d) RECONSTRUCT (LLM, aveugle au texte original): ecrit l'anglais a partir
#         du CSTL (toujours avec les tokens opaques dedans)
#     (e) RESTORE_TEXT (deterministe, orchestrateur): remplace les tokens par les
#         vraies valeurs dans la reconstruction finale -- AVANT de montrer quoi
#         que ce soit aux juges
#   Sort trois fichiers:
#     - kappa_v2_pipeline_results.json  (audit complet, inclut le taux de
#       LITERAL_LOST -- metrique mecanique, independante du jugement des juges)
#     - judge_prompt_v2.txt             (a coller dans 3 chatbots, meme protocole
#       que la version 1 -- les juges voient le texte ORIGINAL vs la
#       reconstruction APRES restauration, jamais les tokens opaques)
#
# Etape 2/3: identiques a kappa_generate_real.py (juges humains-via-chatbot,
# calcul du kappa de Fleiss dans la conversation Claude).

import os
import json
import re
import time
from pathlib import Path
import anthropic

MODEL = os.getenv("ANTHROPIC_MODEL", "claude-haiku-4-5")
MAX_RETRIES = 3
RETRY_BACKOFF_SECONDS = 5  # ajoute 2026-10-05: run sur 18 items a crashe sur
# complex_005 apres 3 "Reponse vide de l'API" consecutives SANS delai entre
# les tentatives -- aucune chance pour une erreur transitoire (surcharge,
# rate-limit cote API) de se resorber. Backoff lineaire simple (5s, 10s, 15s)
# avant chaque nouvelle tentative, pas avant la premiere.

# ===== SOUS-ENSEMBLE RESTREINT (decision Olivier, 2026-10-05) =====
# Sixieme iteration : relance les MEMES 18 items que le run complet haiku
# (commit 85b63ec) mais sur sonnet-4-5 (via ANTHROPIC_MODEL au lancement,
# PAS en modifiant MODEL ci-dessus), pour comparaison complete modele vs
# modele sur tout le sous-ensemble, pas juste les 5 items d'echec deja
# compares individuellement.
#
# MISE EN GARDE METHODOLOGIQUE (pas de comparaison silencieusement biaisee):
# ce run utilise le prompt ACTUEL, qui inclut REACTS (ajoute en 3105d83,
# APRES le run haiku de 85b63ec). Ce n'est donc PAS une comparaison
# controlee isolant uniquement la variable "modele" -- deux choses changent
# en meme temps entre le run haiku (85b63ec, sans REACTS) et ce run sonnet
# (avec REACTS). Sur medium_003 specifiquement, toute amelioration observee
# ne permettra pas de trancher si elle vient du modele ou du nouvel
# operateur disponible. Les 4 autres items (pas d'usage attendu de REACTS)
# restent une comparaison modele-isolee valide.
RUN_SUBSET = {
    "easy_001", "easy_002", "easy_003", "easy_004", "easy_005",
    "medium_001", "medium_002", "medium_003", "medium_004", "medium_005",
    "complex_001", "complex_003", "complex_004", "complex_005",
    "edge_002", "edge_003", "edge_004", "edge_005",
}

API_KEY = os.environ.get("ANTHROPIC_API_KEY")
if not API_KEY:
    raise RuntimeError("ANTHROPIC_API_KEY n'est pas definie.")
client = anthropic.Anthropic(api_key=API_KEY)

# ===== MEME CORPUS QUE kappa_generate_real.py (fige, ne pas modifier) =====

CORPUS = {
    "easy_001": {"text": "Alice knows Bob. Bob is in Paris.", "diff": "easy"},
    "easy_002": {"text": "The capital of France is Paris.", "diff": "easy"},
    "easy_003": {"text": "Einstein wrote the theory of relativity.", "diff": "easy"},
    "easy_004": {"text": "Water boils at 100 degrees Celsius.", "diff": "easy"},
    "easy_005": {"text": "Dogs are animals.", "diff": "easy"},
    "medium_001": {"text": "Sophie arrived late to meeting because traffic was heavy. She apologized.", "diff": "medium"},
    "medium_002": {"text": "Patient prescribed antibiotics, took for week to recover.", "diff": "medium"},
    "medium_003": {"text": "CEO announced layoffs. Employees were upset about decision.", "diff": "medium"},
    "medium_004": {"text": "Climate change accelerating. Scientists warn of consequences.", "diff": "medium"},
    "medium_005": {"text": "Contract signed by parties, confirming agreement on terms.", "diff": "medium"},
    "complex_001": {"text": "Article 42 GDPR: data controllers must implement measures.", "diff": "complex"},
    "complex_002": {"text": "Hypertension >160 mmHg requires ACE inhibitors or calcium channel blockers.", "diff": "complex"},
    "complex_003": {"text": "Algorithm uses Bayesian optimization Thompson sampling.", "diff": "complex"},
    "complex_004": {"text": "NDA prohibits disclosure without prior written consent.", "diff": "complex"},
    "complex_005": {"text": "Three-stage filtration: 10 microns, membrane 0.1 microns, carbon.", "diff": "complex"},
    "edge_001": {"text": "Alice does not believe Bob left early.", "diff": "edge"},
    "edge_002": {"text": "Every student except Marie passed exam.", "diff": "edge"},
    "edge_003": {"text": "Neither manager nor employee knew about change.", "diff": "edge"},
    "edge_004": {"text": "Hiring Sophie controversial, argued not qualified.", "diff": "edge"},
    "edge_005": {"text": "If Alice and Bob leave, project fails.", "diff": "edge"},
}

# ===== LITTERAUX PROTEGES PAR TEXTE (ecrits a la main, PAS une extraction
# generique -- le corpus est fixe et connu, autant etre exact plutot que de
# faire confiance a un regex generique qui pourrait lui-meme se tromper).
# Chaque entree: sous-chaine EXACTE (respecte la casse) a remplacer par un
# token opaque avant que le LLM ne voie le texte. Liste vide = rien a
# proteger pour ce texte (pas de valeur numerique/technique critique).
PROTECTED_SPANS = {
    "easy_001": [],
    "easy_002": [],
    "easy_003": [],
    "easy_004": ["100 degrees Celsius"],
    "easy_005": [],
    "medium_001": [],
    "medium_002": [],
    "medium_003": [],  # jugement de valeur, pas un litteral -- rien a proteger ici
    "medium_004": [],
    "medium_005": [],
    "complex_001": ["Article 42 GDPR"],
    "complex_002": ["160 mmHg", "ACE inhibitors", "calcium channel blockers"],
    "complex_003": ["Bayesian optimization Thompson sampling"],
    "complex_004": [],
    "complex_005": ["10 microns", "0.1 microns"],
    "edge_001": [],  # negation epistemique -- structure logique, pas un litteral
    "edge_002": [],
    "edge_003": [],
    "edge_004": [],  # jugement de valeur, pas un litteral
    "edge_005": [],
}

RESULTS_FILE = Path("kappa_v2_pipeline_results.json")
JUDGE_PROMPT_FILE = Path("judge_prompt_v2.txt")


def call_claude(prompt: str, max_tokens: int = 400) -> str:
    last_exc = None
    for attempt in range(1, MAX_RETRIES + 1):
        try:
            resp = client.messages.create(
                model=MODEL,
                max_tokens=max_tokens,
                messages=[{"role": "user", "content": prompt}],
            )
            text = "\n".join(b.text for b in resp.content if hasattr(b, "text")).strip()
            if not text:
                raise RuntimeError("Reponse vide de l'API.")
            return text
        except Exception as exc:
            msg = str(exc)
            if any(k in msg.lower() for k in (
                "credit balance", "usage limit", "invalid_request_error",
                "authentication_error", "permission_error",
            )):
                raise RuntimeError(f"Erreur API non-recuperable, arret immediat: {msg}") from exc
            last_exc = exc
            print(f"    [retry {attempt}/{MAX_RETRIES}] {msg}")
            if attempt < MAX_RETRIES:
                delay = RETRY_BACKOFF_SECONDS * attempt
                print(f"    [attente {delay}s avant nouvelle tentative]")
                time.sleep(delay)
    raise RuntimeError(f"Echec apres {MAX_RETRIES} tentatives: {last_exc}")


def make_token(text_id: str, index: int) -> str:
    """Token opaque alphanumerique -- deliberement PAS underscore-separe en
    mots plausibles (ce qui mute, voir les trouvailles de session), un bloc
    alphanumerique compact a la AGT7X_K3 (ce qui resiste)."""
    base = re.sub(r"[^A-Z0-9]", "", text_id.upper())[:4]
    return f"LIT{base}{index}X"


def redact(text: str, spans: list[str]) -> tuple[str, dict[str, str]]:
    """Remplace chaque litteral protege par un token opaque. Ordre de
    remplacement: du plus long au plus court, pour qu'un span court contenu
    dans un span plus long (improbable ici mais pas impossible) ne casse pas
    le remplacement du span long."""
    redacted = text
    token_to_literal: dict[str, str] = {}
    for i, span in enumerate(sorted(spans, key=len, reverse=True)):
        if span not in redacted:
            print(f"    [AVERTISSEMENT] litteral protege introuvable tel quel: {span!r}")
            continue
        token = make_token("SPAN", i) + f"{i}"
        # Token unique garanti par index -- pas de collision possible entre spans.
        token = f"LIT{i}QZK"
        redacted = redacted.replace(span, token, 1)
        token_to_literal[token] = span
    return redacted, token_to_literal


def restore(text: str, token_to_literal: dict[str, str]) -> tuple[str, list[str]]:
    """Remplace chaque token opaque par sa vraie valeur. Retourne aussi la
    liste des tokens ATTENDUS mais ABSENTS du texte (LITERAL_LOST) --
    verification purement mecanique, aucun jugement LLM ou humain."""
    restored = text
    lost = []
    for token, literal in token_to_literal.items():
        if token in restored:
            restored = restored.replace(token, literal)
        else:
            lost.append(token)
    return restored, lost


# ===== CATALOGUE REEL, PAS INVENTE (correctif 2026-10-05) =====
# La version precedente de ce prompt proposait une liste d'exemples
# d'operateurs (KNOWS, LOCATED, WROTE, CAUSES, PRESCRIBED, ANNOUNCED,
# REQUIRES, PROHIBITS, NEGATES, EXCEPT, NEITHER_NOR, CONDITIONAL) --
# verifie apres coup contre src/semantic.rs::OFFICIAL_OPERATORS (commit
# 0bcea13) : PAS UN SEUL de ces mots n'est un operateur officiel CSTL, ni
# dans le noyau, ni dans l'extension de domaine medical
# (src/domains.rs::get_domain_operators_slice). Les payloads generes
# passaient quand meme (E101 est un avertissement, jamais bloquant) --
# mais c'etait du CSTL non-conforme qui ressemble a du CSTL, pas la grammaire
# reelle. Liste ci-dessous recopiee a la main depuis
# src/semantic.rs::OFFICIAL_OPERATORS (38 operateurs, verifies le
# 2026-10-05) -- a resynchroniser manuellement si ce fichier evolue, ce
# script n'a pas acces au depot Rust pour l'importer dynamiquement.
OFFICIAL_OPERATORS_SNAPSHOT = [
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST", "AMP", "INH",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES",
    "COMPARES", "ENTAILS", "CONTRADICTS",
    "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS", "DISBELIEVES",
    "BEFORE", "AFTER", "DURING", "EITHER_OR", "REACTS",
]
# Domaine medical (src/domains.rs) -- accepte EN PLUS du noyau quand le
# texte est clairement clinique (c'est le cas de complex_002). Verbes en
# francais dans le depot (ontologie d'origine), gardes tels quels.
# Resynchronise le 2026-10-05 (commit 7fc522b a traduit domains.rs en
# anglais -- cette snapshot etait restee en francais, desynchronisee,
# exactement le genre d'erreur que la convention OFFICIAL_OPERATORS_SNAPSHOT
# existe pour eviter. A resynchroniser manuellement a chaque evolution de
# src/domains.rs, meme avertissement que ci-dessus.
MEDICAL_DOMAIN_OPERATORS_SNAPSHOT = [
    "PRESCRIBE", "DIAGNOSE", "CONTRAINDICATE", "ADMINISTER",
    "OPERATE", "MONITOR", "REFER", "HOSPITALIZE", "TREAT",
    "VACCINATE", "PREVENT",
]


def encode_to_cstl(redacted_text: str) -> str:
    ops = ", ".join(OFFICIAL_OPERATORS_SNAPSHOT)
    med_ops = ", ".join(MEDICAL_DOMAIN_OPERATORS_SNAPSHOT)
    prompt = f"""Represent the factual content of the following English text as a minimal CSTL payload.

Use ONLY these blocks:
DEFINE <entity> AS <type> [id=eNNN]
RELATIONS [
(subject) OPERATOR object [id=rNNN]
]
CONSTRAINTS [
(MODALITY) subject OPERATOR object [id=cNNN]
]

OPERATOR must be chosen from this EXACT closed list (CSTL's real operator catalogue -- do not invent, do not use English verbs not on this list):
{ops}

If the text is clearly clinical/medical, you may ALSO use these domain-specific operators (part of CSTL's medical domain extension), in addition to the list above:
{med_ops}

If the English text expresses an obligation or requirement ("must", "requires", "is required to"), do NOT invent a RELATIONS operator like REQUIRES. Instead use the CONSTRAINTS block with MODALITY=REQUIRE (other valid modalities: MUST, MUST_NOT, NOT, MAY, SHOULD, IF, IFF, UNLESS, FORBID), wrapping an operator from the lists above -- e.g. (REQUIRE) subject ADMINISTRER object [id=cNNN].

If the English text expresses an alternative ("A or B", "either A or B"), do NOT collapse it into two separate parallel relations/constraints (that reads back as "both A and B", a conjunction -- wrong). Instead, after declaring the two relevant constraint/relation lines, add ONE additional RELATIONS line explicitly linking their two objects with EITHER_OR: (object_A) EITHER_OR (object_B) [id=rNNN]. This is the ONLY correct way to express disjunction in CSTL.

If an entity carries a concrete value from the source text (a number, a threshold, a quantity, a named amount, a date) -- including an opaque placeholder token standing in for one -- do NOT leave it implicit in just the entity's name or type. Attach it explicitly with a `value=` attribute on its DEFINE line, e.g. DEFINE Hypertension AS condition [id=e1, value=LIT2QZK]. A DEFINE for an entity that has a concrete value in the source text but no `value=` attribute is INCOMPLETE -- this is a known failure mode, do not repeat it.

If the source text has an adverbial/manner/relative-temporal modifier on an event or relation ("early", "late", "quickly", "reluctantly", etc.), do NOT force it into a BEFORE/AFTER/DURING relation (those are Allen temporal relations between two intervals -- they require a real second term to compare against, and using them for a bare modifier like "early" with nothing to compare to is a type error that silently drops the modifier's actual meaning). Instead attach it as a `manner=` attribute directly on the entity or relation it modifies, e.g. DEFINE left AS event [id=e3, manner=early].

If the source text expresses an epistemic attitude (BELIEVES, KNOWS, DOUBTS, DISBELIEVES, ASSUMES) toward a PROPOSITION (something happening/being true), the object of that operator must be the entity/event that IS the proposition (e.g. `left`, already DEFINEd), never a bare agent name alone (e.g. `Bob`) -- "Alice does not believe Bob" (object=Bob, an agent) and "Alice does not believe [that] Bob left [early]" (object=left, a proposition/event) are different claims; only the second matches what these sentences normally mean. Pick the DEFINEd entity that represents the actual proposition as the object, not whichever agent happens to be nearby in the sentence.

If the source text expresses an EMOTIONAL/AFFECTIVE stance of a subject toward an object or event that already exists (e.g. "upset about", "angry about", "pleased with", "worried about"), use REACTS -- do NOT use CATALYZE (that implies the subject's state causally PRODUCES the object, inverting the direction when the object already happened) and do NOT use BELIEVES/KNOWS (those are epistemic/cognitive, not affective). Pattern: (subject) REACTS (object) [id=rNNN, valence=negative|positive|neutral|mixed, affect=<short free-text label, e.g. upset/angry/pleased>]. valence is required; affect is optional but encouraged when the source text names the specific emotion.

Do not add prose, do not add a hashbang, do not add META. Output ONLY the DEFINE/RELATIONS/CONSTRAINTS blocks that are actually needed (omit a block entirely if the text needs none of it).

IMPORTANT: the text below may contain tokens that look like LIT0QZK, LIT1QZK, etc. These are OPAQUE PLACEHOLDERS for values you cannot see. Copy them EXACTLY, character-for-character, wherever they appear -- never translate, paraphrase, explain, or guess what they might represent.

TEXT:
{redacted_text}"""
    return call_claude(prompt, max_tokens=500)


def reconstruct_from_cstl(cstl_payload: str) -> str:
    prompt = f"""You are given a CSTL payload (a structured semantic representation). You have NEVER seen the original text it came from.

Write a single, natural, standalone English sentence (or two, if needed) that expresses exactly what this payload states -- nothing more, nothing less. Do not mention CSTL, DEFINE, RELATIONS, CONSTRAINTS, or any formatting. Just the plain-English reconstruction.

Reading rules, important, do not default to the wrong one:
- A CONSTRAINTS line with MODALITY=REQUIRE (or MUST) means that object is REQUIRED/obligatory.
- Two separate CONSTRAINTS/RELATIONS lines that are NOT linked by an EITHER_OR line are each independently required -- read them as "and" (conjunction), e.g. "requires both X and Y".
- If (and only if) two objects are explicitly linked by an EITHER_OR relation line, read THOSE TWO as alternatives -- "requires X or Y" (disjunction), NOT "both X and Y". The EITHER_OR line overrides the default conjunctive reading for exactly the two objects it names.
- A REACTS relation means the subject has an emotional/affective stance TOWARD the object -- the object is NOT caused or produced by the subject's state, it's the pre-existing target of the reaction. Reconstruct as "<subject> is/are <affect, or a generic word matching valence if affect is absent> about <object>" -- never phrase it as the subject producing, making, or causing the object.
- If a DEFINE line has a `value=` attribute, that is concrete data from the source text (a number, threshold, quantity, date) and MUST appear in your reconstruction wherever that entity is mentioned -- do not drop it, do not reconstruct the entity as if it were a bare unqualified concept.
- If a DEFINE or RELATIONS line has a `manner=` attribute, that is an adverbial/manner/relative-temporal modifier (e.g. "early", "quickly") and MUST appear in your reconstruction attached to the entity/event it modifies -- do not drop it.

IMPORTANT: the payload may contain tokens that look like LIT0QZK, LIT1QZK, etc. These are OPAQUE PLACEHOLDERS. Copy them EXACTLY, character-for-character, into your reconstruction wherever the meaning calls for that value -- never translate, paraphrase, explain, or guess what they might represent.

CSTL PAYLOAD:
{cstl_payload}"""
    return call_claude(prompt, max_tokens=200)


def main():
    # Fusionne avec les resultats existants au lieu d'ecraser (ajoute
    # 2026-10-05) -- sans ca, relancer un RUN_SUBSET restreint (ex: les 5
    # items d'echec pour comparaison sonnet vs haiku) effacerait
    # silencieusement les resultats deja obtenus et audites pour les items
    # hors du nouveau sous-ensemble. Les cles du RUN_SUBSET courant sont
    # toujours ecrasees par le nouveau run (c'est le but), les autres cles
    # existantes sont preservees telles quelles.
    results = {}
    if RESULTS_FILE.exists():
        try:
            results = json.loads(RESULTS_FILE.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError) as exc:
            print(f"[AVERTISSEMENT] impossible de charger {RESULTS_FILE} existant ({exc}) -- redemarre a vide.")
            results = {}
    total_literals = 0
    total_lost_cstl = 0
    total_lost_reconstruction = 0

    corpus_subset = {k: v for k, v in CORPUS.items() if k in RUN_SUBSET}

    print(f"Modele: {MODEL}")
    print(f"Corpus: {len(corpus_subset)} textes (sous-ensemble restreint: {sorted(RUN_SUBSET)})\n")

    for i, (text_id, entry) in enumerate(corpus_subset.items(), 1):
        original = entry["text"]
        spans = PROTECTED_SPANS.get(text_id, [])
        print(f"[{i}/{len(corpus_subset)}] {text_id} ({entry['diff']}, {len(spans)} litteral(aux) protege(s))")

        redacted_text, token_map = redact(original, spans)
        total_literals += len(token_map)

        print("    encode -> CSTL (litteraux masques)...")
        cstl = encode_to_cstl(redacted_text)

        # Verification mecanique #1: les tokens ont-ils survecu jusqu'au CSTL ?
        lost_in_cstl = [t for t in token_map if t not in cstl]
        total_lost_cstl += len(lost_in_cstl)
        if lost_in_cstl:
            print(f"    [LITERAL_LOST @ CSTL] {lost_in_cstl}")

        print("    reconstruct <- CSTL (a l'aveugle)...")
        reconstruction_redacted = reconstruct_from_cstl(cstl)

        # Verification mecanique #2 + restauration finale -- purement
        # deterministe, aucun LLM/humain implique dans cette etape.
        reconstruction, lost_in_reconstruction = restore(reconstruction_redacted, token_map)
        total_lost_reconstruction += len(lost_in_reconstruction)
        if lost_in_reconstruction:
            print(f"    [LITERAL_LOST @ reconstruction] {lost_in_reconstruction}")

        results[text_id] = {
            "diff": entry["diff"],
            "original": original,
            "protected_spans": spans,
            "redacted_text": redacted_text,
            "cstl_redacted": cstl,
            "reconstruction_redacted": reconstruction_redacted,
            "reconstruction": reconstruction,
            "literal_lost_at_cstl": lost_in_cstl,
            "literal_lost_at_reconstruction": lost_in_reconstruction,
        }

        RESULTS_FILE.write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")
        print(f"    reconstruction (litteraux restaures): {reconstruction[:80]}...\n")

    # ===== metrique mecanique independante, calculee ICI, pas par un juge =====
    literal_preservation_rate = (
        1.0 - (total_lost_cstl + total_lost_reconstruction) / (2 * total_literals)
        if total_literals > 0 else None
    )
    print("=" * 60)
    print(f"Litteraux proteges au total: {total_literals}")
    print(f"Perdus a l'etape CSTL: {total_lost_cstl}")
    print(f"Perdus a l'etape reconstruction: {total_lost_reconstruction}")
    if literal_preservation_rate is not None:
        print(f"Taux de preservation litterale (mecanique, independant des juges): {literal_preservation_rate:.1%}")
    print("=" * 60)

    # ===== prompt-juge consolide, MEME format que kappa_generate_real.py --
    # les juges voient le texte original vs la reconstruction APRES
    # restauration des vraies valeurs, jamais les tokens opaques.
    lines = []
    lines.append("You are evaluating whether meaning was preserved through a text transformation pipeline.")
    lines.append(f"For each of the {len(results)} items below, you are shown TEXT A (original) and TEXT B (a reconstruction).")
    lines.append("Judge ONLY whether TEXT B preserves the essential factual/logical meaning of TEXT A -- ignore style, wording, or phrasing differences.")
    lines.append("")
    lines.append("Answer with EXACTLY one line per item, in this format, nothing else:")
    lines.append("<item_id>: PRESERVED")
    lines.append("or")
    lines.append("<item_id>: NOT_PRESERVED")
    lines.append("")
    lines.append(f"Output all {len(results)} lines, in the order given, and nothing before or after them.")
    lines.append("")
    lines.append("=" * 60)
    lines.append("")

    for text_id, r in results.items():
        lines.append(f"[{text_id}]")
        lines.append(f"TEXT A: {r['original']}")
        lines.append(f"TEXT B: {r['reconstruction']}")
        lines.append("")

    JUDGE_PROMPT_FILE.write_text("\n".join(lines), encoding="utf-8")

    print(f"Audit complet: {RESULTS_FILE}")
    print(f"Prompt a coller dans les 3 chatbots: {JUDGE_PROMPT_FILE}")
    print("Rappel: edge_001 n'a AUCUN litteral protege -- si le desaccord y persiste,")
    print("c'est un vrai signal sur la negation epistemique, pas corrige par ce script.")


if __name__ == "__main__":
    main()
