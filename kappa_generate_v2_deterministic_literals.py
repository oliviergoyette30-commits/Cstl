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
# Dixieme iteration : relance de CONSOLIDATION sur les 13 items du
# sous-ensemble de 18 qui n'ont PAS encore ete retestes individuellement
# avec le prompt final (celui qui inclut toutes les instructions ajoutees
# au fil du diagnostic : catalogue reel, EITHER_OR objet ET sujets,
# value=, manner= adverbial ET predication intransitive,
# PERFORM+STATE pour WARN, FORBID+UNLESS, selection d'objet epistemique,
# REACTS). Exclus de ce lot, deja valides individuellement avec cette
# version finale du prompt (voir
# claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md pour le detail complet
# de chaque test) :
#   - medium_003  (commit f7a9d0a puis a37c931 -- REACTS)
#   - complex_001 (commit f7a9d0a puis a37c931 -- litteral + PERFORM)
#   - complex_004 (commit 2b6fe4a -- FORBID+UNLESS)
#   - edge_005    (commit 6557ea3 -- EITHER_OR sujets)
#   - medium_004  (commit 7cc372b -- manner= intransitif + WARN)
# Lance sur sonnet-4-5 par defaut cette fois (pas de comparaison modele
# recherchee ici, juste la version de production la plus fiable --
# ANTHROPIC_MODEL=claude-sonnet-4-5 au lancement).
# TEST CIBLE INVOLVES/SATISFIES (correctif 2026-10-05, commit a0b4889) --
# les 5 items ou POSSESSES etait mesure comme surcharge (voir
# CSTL_SPEC_v5_0.md §10.3quater) : medium_002/medium_005/edge_004/
# complex_001 (role thematique evenement->argument, devrait migrer vers
# INVOLVES) et complex_004 (satisfaction de precondition, devrait migrer
# vers SATISFIES). A lancer sur les DEUX modeles (haiku par defaut, puis
# ANTHROPIC_MODEL=claude-sonnet-4-5) -- meme methode que les tests
# UNLESS/EITHER_OR/manner= precedents : ne pas conclure que le split
# regle quoi que ce soit avant de verifier que les deux modeles migrent
# spontanement sans que le prompt leur force la main plus qu'il ne le
# fait deja (l'instruction ajoutee au commit a0b4889 dit explicitement
# "ne pas utiliser POSSESSES ici", donc cette fois l'instruction EST deja
# dans le prompt -- ce test verifie qu'elle est suivie, pas qu'un angle
# mort est comble).
#
# RETEST apres correctif de surgeneralisation (2026-10-05, post-9f47de2) --
# le premier test a confirme INVOLVES/SATISFIES sur medium_002/medium_005/
# complex_004 (2 modeles), mais sur edge_004 haiku a surgeneralise
# l'instruction et evite POSSESSES meme pour l'ascription de qualite
# legitime ((hiring) POSSESSES (controversial) -> remplace par RESEMBLES,
# pire semantiquement). Le prompt a ete reecrit pour dire explicitement
# que POSSESSES reste correct pour possession/qualite, avec le cas
# (hiring) POSSESSES (controversial) donne comme exemple correct en toutes
# lettres. Memes 5 items relances sur les deux modeles pour confirmer (a)
# qu'edge_004 garde POSSESSES pour la qualite sur haiku cette fois, et (b)
# qu'aucun des 3 items deja corriges (medium_002/medium_005/complex_004)
# n'a regresse avec la reformulation.
#
# VERIFICATION #3 EN CONDITIONS REELLES (2026-10-05, post-correctif du
# verificateur) -- find_hallucinated_tokens() a ete valide par un test
# synthetique qui rejoue exactement le bug mesure sur medium_002/haiku
# (token LIT0QZK invente sur un item a protected_spans=[]). RUN_SUBSET
# restreint a medium_002 seul, haiku par defaut (c'est le modele ou le
# bug a ete observe) : si le meme defaut se reproduit, la ligne
# [LITERAL_HALLUCINATED @ ...] doit apparaitre dans la sortie -- si
# medium_002 sort propre cette fois (haiku est non-deterministe), ce test
# ne prouve rien dans un sens ou l'autre sur CE run, mais confirme au
# moins que le check ne lance pas de faux positif sur une sortie correcte.
#
# RUN FINAL COMPLET POST-CORRECTIFS (2026-10-05) -- recalcul du kappa de
# Fleiss sur l'ensemble des 18 items avec la version la plus a jour du
# prompt : split POSSESSES/INVOLVES/SATISFIES (a0b4889), correctif de
# sur-generalisation (f8cd9b9), correctif de l'hallucination LITnQZK
# (0e1df56), verification mecanique #3 active (7870d1c). Memes 18 items
# que le run de consolidation precedent (e98e487) -- complex_002 et
# edge_001 toujours exclus (voir claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md
# pour le detail de cette exclusion). Lance sur sonnet-4-5
# (ANTHROPIC_MODEL=claude-sonnet-4-5 au lancement) -- c'est le modele de
# production etabli pour ce run final, pas de comparaison modele
# recherchee ici.
#
# REGRESSION TROUVEE AU RUN CI-DESSUS (commit fb1621f) : edge_002 est
# sorti casse -- "Marie opposes the students arriving to pass the exam"
# -- plus de CONSTRAINTS du tout, le coeur du fait ("students passed
# exam") perdu, remplace par une relation OPPOSES fausse et sans rapport.
# L'instruction de prompt "every X except Y -> CONSTRAINTS[(NOT)...]"
# documentee dans claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md comme
# "confirmee deux fois" ETAIT ABSENTE de ce fichier -- verifie par grep,
# aucune trace. Perdue a un moment non identifie (probablement l'incident
# de regression Termux documente plus tot dans la session), jamais
# retestee depuis faute de RUN_SUBSET repassant par edge_002.
# Instruction rajoutee ci-dessus. RUN_SUBSET restreint a edge_002 seul
# pour verifier le fix avant de relancer les 18 items au complet.
#
# FIX CONFIRME (commit fbc3091) : edge_002 ressort propre --
# RELATIONS[(students) PERFORM passed_exam] + CONSTRAINTS[(NOT) Marie
# PERFORM passed_exam], reconstruction "All the students except Marie
# passed the exam." Relance des 18 items au complet, meme run que
# fb1621f mais avec l'instruction EXCEPT desormais presente.
# RETEST PONCTUEL (2026-10-05, post-c03fae0) -- le run complet ci-dessus a
# donne 100% partout sauf complex_005 : reconstruction coupee net a "The
# filt" (literal_lost_at_reconstruction=['LIT0QZK','LIT1QZK']). Pas un
# defaut de raisonnement -- aucune trace de hallucination/structure
# cassee dans le CSTL lui-meme (propre : 3 stages, INVOLVES+BEFORE
# corrects), juste une reponse coupee en cours de generation (panne
# reseau/API non remontee comme exception par call_claude). RUN_SUBSET
# restreint a cet item seul pour regenerer une reconstruction complete
# avant d'assembler le lot final pour les juges.
# COUCHE RECONSTRUCTION EN PROSE (2026-10-06) -- le recalcul final du kappa
# (3 juges, voir judge_prompt_v2.txt livre) a montre que les desaccords
# residuels ne portent presque plus sur la structure CSTL : medium_005
# ("signed"->"sign") et edge_003 ("knew"->"knows") sont des pertes de temps
# grammatical -- CSTL n'avait jamais ete instruit d'utiliser l'attribut
# tau= (deja present dans la grammaire reelle, src/semantic.rs, jamais
# enseigne dans ce script) ; complex_003 ("uses"->"accesses") vient du
# choix d'operateur ARR.ACCESS, dont la reconstruction par defaut colle au
# nom technique de l'operateur plutot qu'a un verbe neutre. Deux
# correctifs ajoutes : (1) encode_to_cstl enseigne tau=past|present|future
# sur l'evenement quand le temps est marque dans le texte source,
# reconstruct_from_cstl le lit et accorde le verbe en consequence ; (2)
# reconstruct_from_cstl par defaut ARR.ACCESS vers "uses" sauf ressource
# manifestement restreinte. RUN_SUBSET restreint aux 3 items cibles pour
# verifier avant d'etendre a un run complet.
# RETEST #2 (2026-10-06, post-f608c57) -- medium_005 et complex_003 sont
# confirmes corriges (tau=past sur l'evenement signing -> "signed" ;
# ARR.ACCESS evite entierement, PERFORM+INVOLVES choisi a la place ->
# "uses" mot pour mot). edge_003 PAS corrige : KNOWS est une relation
# stative directe sujet->objet, sans entite evenement separee -- la
# premiere formulation de l'instruction tau= ne couvrait que le cas "DEFINE
# d'un evenement", pas celui-la. Instruction elargie pour couvrir aussi
# tau= directement sur une ligne RELATIONS/CONSTRAINTS (KNOWS/BELIEVES/
# POSSESSES/etc. sans evenement intermediaire). RUN_SUBSET restreint a
# edge_003 seul pour verifier ce cas precis avant d'etendre.
RUN_SUBSET = {
    "edge_003",
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


# Forme generale des tokens opaques produits par redact() : LIT<i>QZK, i
# entier. Sert au check de hallucination ci-dessous -- independant de
# combien de spans ont ete reellement proteges pour un item donne.
TOKEN_SHAPE_RE = re.compile(r"LIT\d+QZK")


def find_hallucinated_tokens(text: str, token_to_literal: dict[str, str]) -> list[str]:
    """Verification mecanique #3 (ajoutee 2026-10-05, correctif de l'angle
    mort decouvert sur medium_002/haiku lors du retest INVOLVES/SATISFIES,
    voir claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md) -- les checks #1
    (LITERAL_LOST) et #2 ne detectent que la PERTE d'un token REEL, jamais
    l'INVENTION d'un token de la forme LITnQZK qui n'a jamais ete emis par
    redact() pour cet item (ex: item avec protected_spans=[] -- token_map
    vide -- mais le modele invente quand meme `value=LIT0QZK` dans le CSTL,
    sans aucun antecedent). Un tel token n'est JAMAIS restaure par
    restore() (il ne correspond a aucune cle de token_to_literal), donc il
    reste visible tel quel, non traduit, dans la reconstruction finale --
    un defaut de contenu invisible aux checks precedents. Retourne la liste
    des tokens NON ATTENDUS trouves dans `text` (forme LITnQZK presente,
    mais absente de token_to_literal)."""
    found = set(TOKEN_SHAPE_RE.findall(text))
    return sorted(found - set(token_to_literal))


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
# src/semantic.rs::OFFICIAL_OPERATORS (41 operateurs, verifies le
# 2026-10-05, resynchronise apres ajout de INVOLVES/SATISFIES -- split de
# POSSESSES, voir claude/KAPPA_V2_18ITEMS_FINDINGS_2026-10-05.md et
# CSTL_SPEC_v5_0.md §10.3quater) -- a resynchroniser manuellement si ce
# fichier evolue, ce script n'a pas acces au depot Rust pour l'importer
# dynamiquement.
OFFICIAL_OPERATORS_SNAPSHOT = [
    "ARR", "ARR.CREATE", "ARR.JOIN", "ARR.PRODUCE", "ARR.ACCESS",
    "INTENT", "MAINTAIN", "TRANSFORM", "RESIST", "AMP", "INH",
    "PRESSURE", "CATALYZE", "TRANSMIT_FAITHFUL", "TRANSMIT_INFER",
    "COMMAND", "ASK", "STATE", "PERFORM", "RECOMMEND",
    "EQUALS", "POSSESSES", "RESEMBLES", "CO_LOCATES", "OPPOSES",
    "COMPARES", "ENTAILS", "CONTRADICTS",
    "KNOWS", "BELIEVES", "ASSUMES", "DOUBTS", "DISBELIEVES",
    "BEFORE", "AFTER", "DURING", "EITHER_OR", "REACTS",
    "INVOLVES", "SATISFIES",
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

This EITHER_OR default also applies when MULTIPLE SUBJECTS jointly participate in or trigger the SAME event/object (e.g. "If Alice and Bob leave, the project fails"): declare one RELATIONS line per subject into that same event id (e.g. (Alice) ARR leaving [id=r1] / (Bob) ARR leaving [id=r2]) -- by DEFAULT, with no EITHER_OR line linking the two subjects, this means ALL of them jointly (a conjunction, "Alice AND Bob leave"), exactly the same default-conjunctive reading as any other pair of parallel relation/constraint lines. Only if the source text says "either X or Y" ABOUT THE SUBJECTS THEMSELVES do you add (Alice) EITHER_OR (Bob) [id=rNNN] to override that default to a disjunction ("Alice OR Bob leaves"). Do not guess -- the presence or absence of this EITHER_OR line between the subjects is what decides AND vs OR, never infer it from word order or plausibility.

If an entity carries a concrete value from the source text (a number, a threshold, a quantity, a named amount, a date), do NOT leave it implicit in just the entity's name or type. Attach it explicitly with a `value=` attribute on its DEFINE line, using the EXACT text that already appears in the source text you were given -- e.g. if the source text literally contains the characters "LIT2QZK" (an opaque placeholder already present in the input), write value=LIT2QZK; if the source text says "a week", write value=a_week or value=week (plain words from the input, with spaces replaced by underscores if needed), never a LITnQZK-shaped token. A DEFINE for an entity that has a concrete value in the source text but no `value=` attribute is INCOMPLETE -- this is a known failure mode, do not repeat it.

CRITICAL: tokens of the exact shape LITnQZK (a number between LIT and QZK, e.g. LIT0QZK, LIT1QZK) are NOT a generic notation for "any numeric or quantity value" -- they are opaque placeholders that the surrounding pipeline inserts into the input text BEFORE you see it, standing in for a specific literal span that was masked out. You must NEVER invent, write, or use a LITnQZK-shaped token yourself unless that EXACT token already appears, character-for-character, in the source text you were given. If the source text you received contains no token of the shape LITnQZK anywhere in it, do not write one anywhere in your output -- not as a value=, not anywhere -- even if the text mentions a quantity, a duration, or a number in plain words (e.g. "a week", "three days", "100 degrees"). Plain-word quantities like these get a plain-word value= (value=a_week), never a fabricated LITnQZK token. Writing a LITnQZK-shaped token that was not already in the input is a serious error -- it fabricates a reference to a masked literal that was never masked, and it will show up unresolved and meaningless in the final output.

If the source text has an adverbial/manner/relative-temporal modifier on an event or relation ("early", "late", "quickly", "reluctantly", etc.), do NOT force it into a BEFORE/AFTER/DURING relation (those are Allen temporal relations between two intervals -- they require a real second term to compare against, and using them for a bare modifier like "early" with nothing to compare to is a type error that silently drops the modifier's actual meaning). Instead attach it as a `manner=` attribute directly on the entity or relation it modifies, e.g. DEFINE left AS event [id=e3, manner=early].

If the source text says a subject is undergoing a change or has a changing property ON ITS OWN, with no second entity actually being acted upon ("climate change is accelerating", "the situation is worsening", "the economy is growing") -- do NOT invent a second entity for the property/change and link the subject to it with TRANSFORM or any other transitive RELATIONS operator (that falsely implies the subject is acting on some separate object). This is a self-change, not a two-party relation. Instead attach the change/property as a `manner=` attribute directly on the subject's own DEFINE line, the same mechanism as the adverbial modifier case above, e.g. DEFINE climate_change AS phenomenon [id=e1, manner=accelerating].

If the source text describes someone WARNING about something (as opposed to merely stating or declaring it neutrally) -- there is no dedicated WARN operator in the closed list above, and using STATE alone drops the cautionary/urgent illocutionary force of "warn". Instead compose it from two relations: PERFORM for the act of warning as an event, then STATE from that event to its content, e.g. DEFINE warning AS event [id=e2] / RELATIONS [ (scientists) PERFORM warning [id=r1] (warning) STATE consequences [id=r2] ]. This preserves "warn" as a distinct event rather than flattening it into a neutral declarative.

If the source text expresses an epistemic attitude (BELIEVES, KNOWS, DOUBTS, DISBELIEVES, ASSUMES) toward a PROPOSITION (something happening/being true), the object of that operator must be the entity/event that IS the proposition (e.g. `left`, already DEFINEd), never a bare agent name alone (e.g. `Bob`) -- "Alice does not believe Bob" (object=Bob, an agent) and "Alice does not believe [that] Bob left [early]" (object=left, a proposition/event) are different claims; only the second matches what these sentences normally mean. Pick the DEFINEd entity that represents the actual proposition as the object, not whichever agent happens to be nearby in the sentence.

If the source text expresses an EMOTIONAL/AFFECTIVE stance of a subject toward an object or event that already exists (e.g. "upset about", "angry about", "pleased with", "worried about"), use REACTS -- do NOT use CATALYZE (that implies the subject's state causally PRODUCES the object, inverting the direction when the object already happened) and do NOT use BELIEVES/KNOWS (those are epistemic/cognitive, not affective). Pattern: (subject) REACTS (object) [id=rNNN, valence=negative|positive|neutral|mixed, affect=<short free-text label, e.g. upset/angry/pleased>]. valence is required; affect is optional but encouraged when the source text names the specific emotion.

If the source text expresses a prohibition that has a named EXCEPTION ("X is prohibited without Y", "X is forbidden unless Y", "not allowed except with Y"), do NOT invent a RELATIONS operator that doesn't exist in the closed list above (e.g. there is no "PROHIBITS"), and do NOT misuse a temporal operator like BEFORE/AFTER/DURING to express the exception (those are Allen temporal relations between two time intervals, not a conditional-exception relation -- "without prior written consent" is not a claim about temporal ordering). Instead use TWO CONSTRAINTS lines: one with MODALITY=FORBID for the base prohibition, and one with MODALITY=UNLESS naming the exception condition as its object, both using an operator from the closed list above -- e.g. CONSTRAINTS [ (FORBID) subject PERFORM disclosure [id=c1] (UNLESS) disclosure SATISFIES consent [id=c2] ]. This is the ONLY correct way to express a conditional exception to a prohibition in CSTL.

If the source text makes an AFFIRMATIVE universal claim with a single named exception ("every X except Y", "all X but Y", "none of the X except Y", "X, with the exception of Y"), do NOT invent an unrelated RELATIONS operator for the excepted entity (e.g. do not use OPPOSES, CONTRADICTS, or anything implying conflict -- the source text says nothing about Y opposing or disagreeing with anything, it just says Y is the one exception to an otherwise-universal fact), and do NOT drop the general claim about the rest of the group. Instead: (1) in RELATIONS, state the general fact holding for the whole group exactly as you would if there were no exception at all (e.g. (students) PERFORM passed_exam); (2) in CONSTRAINTS, add ONE line with MODALITY=NOT naming the excepted individual and the SAME event/property being negated for them specifically (e.g. (NOT) Marie PERFORM passed_exam). Do not add any other relation involving the excepted individual beyond this one CONSTRAINTS line -- in particular, never add a RELATIONS line claiming the excepted individual belongs to, possesses, or is connected to the group in any other way; the CONSTRAINTS (NOT) line alone is sufficient and correct. Pattern: RELATIONS [ (students) PERFORM passed_exam [id=r1] ] CONSTRAINTS [ (NOT) Marie PERFORM passed_exam [id=c1] ].

POSSESSES remains the CORRECT and NORMAL operator for literal possession and for attribute/quality ascription -- do not avoid it for this. Use it whenever an entity (person, document, event, anything) HAS a property, quality, or value directly, e.g. (patient) POSSESSES (risk), (renal_function) POSSESSES (value) [UNKNOWN=true], or (hiring) POSSESSES (controversial) -- that last one is correct even though "hiring" is an event, because "controversial" is a quality being ascribed to it, not a participant in it. Only TWO specific narrow cases should use a different operator instead of POSSESSES, and ONLY these two: (1) when the SUBJECT is an EVENT and the OBJECT is a separate PARTICIPANT in that event -- someone or something the event involves, acts upon, or is about, as opposed to a quality of the event itself (e.g. a signing event and the contract it is about, a "took medication" event and the medication/duration involved, a hiring event and the person who got hired) -- use INVOLVES instead, e.g. (signing) INVOLVES (contract) [id=rNNN, role=theme]; (2) when the relation expresses that some condition or authorization has been MET or OBTAINED (e.g. "has consent", "meets the requirement") rather than owning a thing -- use SATISFIES instead, e.g. (UNLESS) disclosure SATISFIES consent [id=cNNN]. Outside these two narrow cases, keep using POSSESSES exactly as before -- do not generalize away from it for ordinary possession or quality ascription.

If the source text's main event/action has a grammatical tense that is clearly marked by the verb form (e.g. "signed" = past, "signs"/"is signing" = present, "will sign" = future), attach it explicitly as a `tau=` attribute (an existing CSTL attribute, not a new one), using one of exactly three values: tau=past, tau=present, or tau=future. There are two cases depending on HOW the verb is expressed: (1) if the verb corresponds to a separate DEFINEd event entity (e.g. "signing" in "parties PERFORM signing"), attach `tau=` on that event's DEFINE line, e.g. DEFINE signing AS event [id=e2, tau=past]; (2) if the verb is instead a STATIVE/EPISTEMIC relation used directly between subject and object with no separate event entity (e.g. KNOWS, BELIEVES, POSSESSES, DOUBTS, ASSUMES, DISBELIEVES -- "the manager knew", "she believes", "it possessed") -- attach `tau=` directly on that RELATIONS or CONSTRAINTS line itself, e.g. CONSTRAINTS [ (NOT) manager KNOWS change [id=c1, tau=past] ]. Do not skip tau= just because there is no event entity to hang it on -- the relation/constraint line itself always accepts the same attribute. CSTL itself carries no tense information unless you attach it this way -- omitting `tau=` when the source text clearly marks a tense is a loss of information, the same class of omission as leaving out a `value=` or `manner=` that the source text clearly provides. If the source text is a tenseless general statement (a definition, a standing rule, "dogs are animals") with no single marked event tense, omit `tau=` entirely -- do not force a tense onto a timeless claim.

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
- The same rule applies when multiple subjects each have their own relation line into the same shared event/object: with NO EITHER_OR line linking those subjects, read them as jointly/ALL of them (conjunction -- "Alice and Bob leave", never "Alice or Bob leave"). Only read it as a disjunction ("Alice or Bob leave") if an EITHER_OR line explicitly links those subjects. Do not default to "or" just because two separate lines point to the same object -- the default is always "and" unless EITHER_OR says otherwise.
- A REACTS relation means the subject has an emotional/affective stance TOWARD the object -- the object is NOT caused or produced by the subject's state, it's the pre-existing target of the reaction. Reconstruct as "<subject> is/are <affect, or a generic word matching valence if affect is absent> about <object>" -- never phrase it as the subject producing, making, or causing the object.
- A MODALITY=UNLESS line paired with a MODALITY=FORBID (or MUST_NOT) line on the same subject/action means the prohibition does NOT apply when the UNLESS line's condition holds -- reconstruct as "X is forbidden/prohibited unless Y" or "X is not allowed without Y", never as a temporal claim ("before Y") and never drop the exception entirely.
- A MODALITY=NOT CONSTRAINTS line naming one specific individual, paired with a RELATIONS line stating the same event/property for a whole group that individual is part of, means "every member of the group except that individual" -- reconstruct as "every/all <group> except <individual> <verb-phrase>" (or equivalent), keeping BOTH the general claim about the group AND the named exception. Do not phrase the excepted individual as opposing, conflicting with, or otherwise relating to the group in any other way than being the one exception -- the source relation is purely "excluded from this one fact", nothing more.
- If a DEFINE line has a `value=` attribute, that is concrete data from the source text (a number, threshold, quantity, date) and MUST appear in your reconstruction wherever that entity is mentioned -- do not drop it, do not reconstruct the entity as if it were a bare unqualified concept.
- If a DEFINE or RELATIONS line has a `manner=` attribute, that is an adverbial/manner/relative-temporal modifier OR a self-change/property the entity itself is undergoing (e.g. "early", "quickly", "accelerating", "worsening") and MUST appear in your reconstruction attached to the entity/event it modifies -- do not drop it, and do not phrase a self-change `manner=` as the entity acting on some other thing.
- If an entity is PERFORMed as an event and that event in turn STATEs an object (a two-step chain: subject PERFORM event, event STATE content), and the event's own name/type suggests warning/cautioning rather than a neutral announcement, reconstruct it with "warn" (or an equivalent cautionary verb), not a flat neutral "state"/"say" -- preserve the urgency, don't flatten it to a bare declarative.
- An INVOLVES relation means the subject (normally an event) has the object as its participant/theme/patient/duration -- reconstruct it as a normal verb phrase binding the event to that argument (e.g. "the parties sign the contract", "the patient took the antibiotics for a week"), never as "possesses"/"has" (INVOLVES is not possession).
- A SATISFIES relation means the subject meets/fulfills the object as a condition or authorization -- reconstruct it as "has met/obtained/fulfilled <object>" or similar (e.g. "unless it has obtained prior written consent"), never as "possesses"/"has" in the literal-ownership sense.
- If an event's DEFINE line, OR the RELATIONS/CONSTRAINTS line itself (for a stative/epistemic relation like KNOWS/BELIEVES/POSSESSES used directly between subject and object, with no separate event entity), carries a `tau=` attribute, render that verb phrase in the matching grammatical tense: tau=past -> simple past ("signed", "knew"), tau=present -> simple present ("signs", "knows"), tau=future -> "will" + base form ("will sign", "will know"). Check BOTH places for `tau=` -- it is not always on the same line. If neither carries a `tau=` attribute, default to simple present, exactly as before -- do not guess a tense that isn't marked.
- An ARR.ACCESS relation, by itself, does not necessarily mean "accesses" in the narrow technical sense (reaching a restricted system, file, or permission) -- it is also used for an agent employing/using a tool, method, or resource more generally. Unless the object is clearly a restricted/gated resource (a system, an account, a permission), reconstruct ARR.ACCESS with the neutral verb "uses" rather than "accesses" -- "uses" is the more natural default reading for this relation.

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
    total_hallucinated_cstl = 0
    total_hallucinated_reconstruction = 0
    items_with_hallucination = []

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

        # Verification mecanique #3a: le modele a-t-il INVENTE un token
        # LITnQZK sans antecedent (voir find_hallucinated_tokens ci-dessus) ?
        hallucinated_in_cstl = find_hallucinated_tokens(cstl, token_map)
        total_hallucinated_cstl += len(hallucinated_in_cstl)
        if hallucinated_in_cstl:
            print(f"    [LITERAL_HALLUCINATED @ CSTL] {hallucinated_in_cstl} -- token sans antecedent dans protected_spans")

        print("    reconstruct <- CSTL (a l'aveugle)...")
        reconstruction_redacted = reconstruct_from_cstl(cstl)

        # Verification mecanique #3b: meme chose cote reconstruction, AVANT
        # restore() -- un token invente ici ne correspond a aucune cle de
        # token_map, restore() le laisse donc tel quel (non traduit) dans le
        # texte final, ce que #3a seul ne capturerait pas si le modele
        # l'avait invente seulement a cette etape-ci plutot qu'au CSTL.
        hallucinated_in_reconstruction = find_hallucinated_tokens(reconstruction_redacted, token_map)
        total_hallucinated_reconstruction += len(hallucinated_in_reconstruction)
        if hallucinated_in_reconstruction:
            print(f"    [LITERAL_HALLUCINATED @ reconstruction] {hallucinated_in_reconstruction} -- token sans antecedent dans protected_spans")

        if hallucinated_in_cstl or hallucinated_in_reconstruction:
            items_with_hallucination.append(text_id)

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
            "literal_hallucinated_at_cstl": hallucinated_in_cstl,
            "literal_hallucinated_at_reconstruction": hallucinated_in_reconstruction,
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
    # Hallucination de token (verification #3, ajoutee 2026-10-05) --
    # independante de total_literals : peut se produire meme sur un item a
    # 0 littteral protege (c'est exactement le cas qui l'a revelee,
    # medium_002/haiku). Affichee meme si total_hallucinated est 0, pour
    # qu'un 0 explicite distingue "verifie, rien trouve" de "jamais verifie".
    total_hallucinated = total_hallucinated_cstl + total_hallucinated_reconstruction
    print(f"Tokens LITnQZK HALLUCINES (sans antecedent, verif #3): {total_hallucinated} (CSTL: {total_hallucinated_cstl}, reconstruction: {total_hallucinated_reconstruction})")
    if items_with_hallucination:
        print(f"  -> items affectes: {items_with_hallucination}")
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
