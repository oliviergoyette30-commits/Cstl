#!/usr/bin/env python3
"""
operator_agreement_study.py -- harnais de mesure pour la question ouverte
listee dans CSTL_SPEC_v5_0.md §23 point 4 : "Mesure accord inter-operateurs
-- quel operateur les LLM choisissent pour un fait donne ?"

CE QUE CE FICHIER EST : un pipeline complet et deterministe --
faits de reference -> prompt standardise -> backend LLM pluggable ->
scoring d'accord inter-annotateurs (accord brut + kappa) -> rapport,
avec les faits les plus "controverses" (le plus de desaccord) en tete.

CE QUE CE FICHIER N'EST PAS (limite honnete, verifiee dans ce sandbox le
2026-09-05) : une mesure empirique reelle. Ni ANTHROPIC_API_KEY, ni
OPENAI_API_KEY, ni GEMINI_API_KEY ne sont presentes dans cet environnement
(verifie avec `echo $ANTHROPIC_API_KEY` etc., toutes vides), et meme si
elles l'etaient, cet outil ne doit pas faire de vrais appels API sans
autorisation explicite de l'utilisateur -- donc AUCUN appel reseau reel
n'a ete fait pendant l'ecriture ou la verification de ce script. Ce qui a
ete verifie ici, en direct, sans reseau : la logique de collecte, de
comparaison et de scoring, via DeterministicStubBackend (reponses fixees
d'avance, connues du testeur) -- voir tests/test_operator_agreement_study.py
et la section AUTO-TEST plus bas. Deux stubs qui repondent toujours pareil
donnent 100% d'accord ; un desaccord force dans le scenario de test est
correctement detecte et baisse le score en consequence (voir le test
`test_forced_disagreement_is_detected`).

Meme patron de degradation propre que AnthropicAgentBrain.from_env()
(sdk/python/cstl_llm_agent.py) / TelegramNotifier::from_env()
(src/telegram_council.rs) : AnthropicBackend.from_env() retourne None
(pas d'exception) si le paquet `anthropic` ou la cle sont absents.

POUR L'UTILISATEUR -- comment produire une VRAIE mesure une fois pret :

    pip install anthropic google-genai     # et/ou openai...
    export ANTHROPIC_API_KEY=sk-...
    # Gemini a un palier gratuit reel (cle sur aistudio.google.com/apikey,
    # aucune carte de credit requise) -- la comparaison inter-modeles la
    # plus accessible sans frais :
    export GEMINI_API_KEY=...

    python3 scripts/operator_agreement_study.py --backends anthropic,gemini

    # Comparaison inter-modeles reelle (le but scientifique de la question
    # posee dans la spec) necessite au moins deux backends reels -- c'est
    # exactement ce que anthropic+gemini ci-dessus donne. Un OpenAIBackend
    # supplementaire est une extension directe : implementer la classe
    # abstraite OperatorChoiceBackend ci-dessous et l'ajouter a
    # BACKEND_REGISTRY, meme patron que GeminiBackend.

    # Mode 100% local, sans reseau, pour verifier que le harnais tourne :
    python3 scripts/operator_agreement_study.py --dry-run
    # ou, equivalent explicite :
    python3 scripts/operator_agreement_study.py --backends stub_a,stub_b

Aucun appel LLM reel n'a ete effectue par ce script dans le depot au
moment de son ecriture -- seul --dry-run / --backends stub* a ete execute
ici. `google-genai` a ete installe et importe avec succes dans ce sandbox
(2026-09-05) pour ecrire et verifier structurellement GeminiBackend, mais
GEMINI_API_KEY est absente ici comme ANTHROPIC_API_KEY -- seule la
degradation propre de from_env() (retourne None) a pu etre verifiee en
direct, jamais un vrai appel a l'API Gemini.
"""

from __future__ import annotations

import argparse
import itertools
import json
import os
import sys
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "sdk" / "python"))


# ---------------------------------------------------------------------------
# Liste des operateurs officiels et leur definition courte -- transcrite
# depuis README.md section "v5.0.0 Operators" (ligne ~231, verifiee a la
# main le 2026-09-05 -- ne PAS dupliquer semantic.rs::OFFICIAL_OPERATORS
# en Rust, ceci est une aide de prompt pour un LLM externe, pas une source
# de verite pour la validation cote serveur). Le fallback RELATE et le
# deprecated MUTUAL sont inclus pour completude du prompt mais exclus de
# la liste "attendue" des faits de reference (un fait bien choisi devrait
# toujours avoir un operateur canonique plus precis).
# ---------------------------------------------------------------------------

OPERATOR_DEFINITIONS: dict[str, str] = {
    # Logical
    "ENTAILS": "A implique logiquement B",
    "CONTRADICTS": "A et B sont logiquement incompatibles",
    # Epistemic
    "KNOWS": "fait etabli avec un haut degre de certitude (sigma eleve)",
    "BELIEVES": "opinion ou croyance, certitude moderee",
    "ASSUMES": "hypothese non verifiee, faible certitude",
    "DOUBTS": "scepticisme explicite envers une proposition",
    # Temporal (Allen 1983 subset)
    "BEFORE": "l'evenement/etat A precede B dans le temps",
    "AFTER": "l'evenement/etat A suit B dans le temps",
    "DURING": "A a lieu pendant l'intervalle de B",
    # Relational
    "EQUALS": "A et B designent la meme entite",
    "POSSESSES": "A possede/detient B",
    "RESEMBLES": "A ressemble a B sans etre identique",
    "CO_LOCATES": "A et B partagent un lieu",
    "OPPOSES": "A s'oppose a B",
    "COMPARES": "A est compare a B (relation de comparaison generique)",
    # Causality
    "ARR": "relation causale generique (A cause B)",
    "ARR.CREATE": "A cree/fait naitre B",
    "ARR.JOIN": "A rejoint/rassemble B",
    "ARR.PRODUCE": "A produit B (resultat materiel ou output)",
    "ARR.ACCESS": "A donne acces a B",
    # Speech acts
    "COMMAND": "A ordonne B a un destinataire",
    "ASK": "A pose une question / requete a propos de B",
    "STATE": "A enonce B comme une affirmation neutre",
    "PERFORM": "A execute/accomplit B (acte performatif)",
    "RECOMMEND": "A recommande B",
    # Intent / dynamics
    "INTENT": "A a l'intention de faire/obtenir B",
    "MAINTAIN": "A maintient un etat B",
    "TRANSFORM": "A transforme B en autre chose",
    "RESIST": "A resiste a B",
    "AMP": "A amplifie B",
    "INH": "A inhibe B",
    "PRESSURE": "A exerce une pression sur B",
    "CATALYZE": "A catalyse/accelere B",
    "TRANSMIT_FAITHFUL": "A transmet B fidelement, sans perte",
    "TRANSMIT_INFER": "A transmet B avec inference/reconstruction",
    # Fallback / deprecated -- inclus dans le prompt pour completude, mais
    # jamais utilises comme operateur "attendu" dans REFERENCE_FACTS.
    "RELATE": "aucun operateur canonique ne convient -- necessite type=custom gloss=...",
}

# Deontic modality (MUST/REQUIRE, MUST_NOT/FORBID) est un attribut
# RELATION.modality separe, pas un operateur RELATION.type -- voir
# README.md et semantic.rs::check_axiom_d. Deliberement absent de la
# liste ci-dessus et des faits de reference : ce n'est pas le meme choix
# que celui etudie ici (choix d'operateur), donc le melanger fausserait
# la mesure d'accord.


def format_operator_menu() -> str:
    lines = [f"- {op}: {definition}" for op, definition in OPERATOR_DEFINITIONS.items()]
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Faits de reference. `expected_operator` est une reference de DEPART
# choisie par un humain (l'auteur de ce script) -- PAS une verite absolue.
# Le but de l'etude n'est pas de verifier que les LLM sont "corrects" par
# rapport a cette colonne, mais de mesurer leur accord MUTUEL ; la colonne
# `expected_operator` sert seulement de point de comparaison optionnel et
# de documentation du raisonnement de depart, discutable comme n'importe
# quelle annotation humaine unique (un seul annotateur, aucune
# double-passation -- limite assumee).
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class ReferenceFact:
    id: str
    category: str
    text: str
    expected_operator: str
    note: str = ""


REFERENCE_FACTS: list[ReferenceFact] = [
    # -- Epistemique --
    ReferenceFact("F01", "epistemic", "Marie Curie a decouvert le radium.", "KNOWS",
                  "fait scientifique etabli, consensus historique fort"),
    ReferenceFact("F02", "epistemic", "Certains chercheurs pensent que la vie existe ailleurs dans l'univers.", "BELIEVES",
                  "opinion partagee, non prouvee"),
    ReferenceFact("F03", "epistemic", "On suppose que ce document date du XIIe siecle, sans datation au carbone confirmee.", "ASSUMES",
                  "hypothese explicitement non verifiee"),
    ReferenceFact("F04", "epistemic", "L'expert doute que ce tableau soit un authentique Vermeer.", "DOUBTS",
                  "scepticisme explicite"),
    # -- Temporel --
    ReferenceFact("F05", "temporal", "La Revolution francaise a eu lieu avant la revolution industrielle en France.", "BEFORE", ""),
    ReferenceFact("F06", "temporal", "La cerimonie a eu lieu apres la signature du traite.", "AFTER", ""),
    ReferenceFact("F07", "temporal", "Le seisme s'est produit pendant la conference internationale.", "DURING", ""),
    # -- Relationnel --
    ReferenceFact("F08", "relational", "Mark Twain et Samuel Clemens sont la meme personne.", "EQUALS", ""),
    ReferenceFact("F09", "relational", "Le musee possede la plus grande collection d'art impressionniste au monde.", "POSSESSES", ""),
    ReferenceFact("F10", "relational", "Ce nouveau modele ressemble beaucoup a son predecesseur, sans etre identique.", "RESEMBLES", ""),
    ReferenceFact("F11", "relational", "Paris et la tour Eiffel se trouvent au meme endroit.", "CO_LOCATES",
                  "cas plus ambigu -- pourrait aussi etre lu comme POSSESSES selon le cadrage"),
    ReferenceFact("F12", "relational", "Le syndicat s'oppose fermement a la nouvelle reforme.", "OPPOSES", ""),
    ReferenceFact("F13", "relational", "Les deux etudes sont comparees dans la meta-analyse.", "COMPARES", ""),
    # -- Causalite --
    ReferenceFact("F14", "causality", "La pollution atmospherique cause une augmentation des maladies respiratoires.", "ARR", ""),
    ReferenceFact("F15", "causality", "L'artiste a cree une nouvelle oeuvre pour l'exposition.", "ARR.CREATE", ""),
    ReferenceFact("F16", "causality", "La fusine a produit dix mille unites ce mois-ci.", "ARR.PRODUCE", ""),
    ReferenceFact("F17", "causality", "Ce badge donne acces a la salle des serveurs.", "ARR.ACCESS", ""),
    # -- Actes de langage --
    ReferenceFact("F18", "speech_act", "Le general a ordonne le retrait immediat des troupes.", "COMMAND", ""),
    ReferenceFact("F19", "speech_act", "Le journaliste a demande au ministre de clarifier sa position.", "ASK", ""),
    ReferenceFact("F20", "speech_act", "Le rapport indique que les ventes ont augmente de 12% ce trimestre.", "STATE",
                  "cas volontairement proche de KNOWS -- STATE est l'acte d'enonciation, KNOWS le statut epistemique du contenu ; distinction subtile, candidate a un fort desaccord inter-LLM"),
    ReferenceFact("F21", "speech_act", "Le comite recommande l'adoption immediate de la nouvelle norme.", "RECOMMEND", ""),
    # -- Intention / dynamique --
    ReferenceFact("F22", "dynamics", "L'entreprise a l'intention d'ouvrir trois nouveaux bureaux l'an prochain.", "INTENT", ""),
    ReferenceFact("F23", "dynamics", "La banque centrale maintient son taux directeur inchange.", "MAINTAIN", ""),
]


# ---------------------------------------------------------------------------
# Interface de backend pluggable. Un backend "choisit" un operateur parmi
# OPERATOR_DEFINITIONS pour un ReferenceFact donne, et retourne une
# justification courte -- sans jamais planter sur une reponse imparfaite
# (un backend reel doit degrader vers une reponse marquee invalide plutot
# que lever une exception non geree, pour que l'etude produise un rapport
# meme si un backend repond mal a certains faits).
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class OperatorChoice:
    operator: str
    justification: str
    raw_response: str = ""


class OperatorChoiceBackend(ABC):
    """Interface abstraite. `name` identifie le backend dans le rapport
    (ex: "claude-sonnet-4-5", "gpt-4o", "stub_a")."""

    name: str

    @abstractmethod
    def choose_operator(self, fact: ReferenceFact) -> OperatorChoice:
        ...


def build_prompt(fact: ReferenceFact) -> str:
    """Prompt standard envoye a chaque backend -- identique pour tous les
    faits et tous les backends, condition necessaire pour que l'accord
    mesure reflete une difference de jugement du modele et non une
    difference de formulation du prompt."""
    return (
        "Tu dois choisir UN SEUL operateur CSTL (protocole de transfert "
        "semantique compresse) pour encoder le fait suivant en langage "
        "naturel :\n\n"
        f'  "{fact.text}"\n\n'
        "Voici la liste complete des operateurs officiels CSTL v5.0.0 et "
        "leur definition courte :\n\n"
        f"{format_operator_menu()}\n\n"
        "Reponds STRICTEMENT en JSON, sur une seule ligne, avec exactement "
        'ces deux cles : {"operator": "<UN_OPERATEUR_DE_LA_LISTE>", '
        '"justification": "<une phrase courte>"}. '
        "N'utilise RELATE que si vraiment aucun autre operateur ne convient."
    )


def _parse_json_choice(text: str) -> OperatorChoice:
    """Parse une reponse JSON stricte {"operator":..., "justification":...}.
    Tolere un texte autour du JSON (certains modeles ajoutent des
    preambules malgre l'instruction) en cherchant la premiere accolade
    ouvrante et la derniere fermante, meme patron que
    AnthropicAgentBrain.generate_relation (sdk/python/cstl_llm_agent.py)."""
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        start, end = text.find("{"), text.rfind("}")
        if start == -1 or end == -1:
            return OperatorChoice(operator="__PARSE_ERROR__", justification="", raw_response=text)
        try:
            obj = json.loads(text[start:end + 1])
        except json.JSONDecodeError:
            return OperatorChoice(operator="__PARSE_ERROR__", justification="", raw_response=text)
    op = str(obj.get("operator", "__MISSING__")).strip()
    just = str(obj.get("justification", "")).strip()
    return OperatorChoice(operator=op, justification=just, raw_response=text)


# ---------------------------------------------------------------------------
# Backend reel -- meme patron from_env() que AnthropicAgentBrain
# (sdk/python/cstl_llm_agent.py) / TelegramNotifier::from_env()
# (src/telegram_council.rs) : degradation propre (retourne None), jamais
# d'exception, si le paquet ou la cle manquent.
# ---------------------------------------------------------------------------

@dataclass
class AnthropicBackend(OperatorChoiceBackend):
    client: object
    model: str
    name: str = field(default="")

    def __post_init__(self):
        if not self.name:
            self.name = f"anthropic:{self.model}"

    @classmethod
    def from_env(cls, model: str = "claude-sonnet-4-5") -> Optional["AnthropicBackend"]:
        try:
            import anthropic  # type: ignore
        except ImportError:
            print("[AnthropicBackend] paquet 'anthropic' absent -- pip install anthropic. Degradation: None.", file=sys.stderr)
            return None
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            print("[AnthropicBackend] ANTHROPIC_API_KEY absent de l'environnement. Degradation: None.", file=sys.stderr)
            return None
        client = anthropic.Anthropic(api_key=api_key)
        return cls(client=client, model=model)

    def choose_operator(self, fact: ReferenceFact) -> OperatorChoice:
        prompt = build_prompt(fact)
        response = self.client.messages.create(
            model=self.model,
            max_tokens=200,
            messages=[{"role": "user", "content": prompt}],
        )
        text = "".join(block.text for block in response.content if hasattr(block, "text"))
        return _parse_json_choice(text)


# ---------------------------------------------------------------------------
# Backend Gemini reel -- meme patron from_env() qu'AnthropicBackend ci-dessus.
# Ajoute le 2026-09-05 : Gemini a un palier gratuit reel (cle API sur
# aistudio.google.com/apikey, aucune carte de credit requise contrairement a
# l'API Anthropic), ce qui rend une VRAIE comparaison inter-modeles
# accessible a l'utilisateur sans frais -- Claude restant disponible via le
# chat (abonnement Max de l'utilisateur), pas via l'API payante.
#
# Paquet `google-genai` (le SDK unifie actuel, PAS l'ancien
# `google-generativeai` deprecie) installe et importe avec succes dans ce
# sandbox (`pip install google-genai`, confirme). Comme pour AnthropicBackend,
# AUCUNE cle (`GEMINI_API_KEY`) n'est presente ici (confirme avec
# `echo $GEMINI_API_KEY`, vide) -- seule la degradation propre de
# `from_env()` (retourne None) a pu etre verifiee en direct dans ce sandbox,
# jamais un vrai appel reseau a l'API Gemini.
# ---------------------------------------------------------------------------

@dataclass
class GeminiBackend(OperatorChoiceBackend):
    client: object
    model: str
    name: str = field(default="")

    def __post_init__(self):
        if not self.name:
            self.name = f"gemini:{self.model}"

    @classmethod
    def from_env(cls, model: str = "gemini-3.6-flash") -> Optional["GeminiBackend"]:
        try:
            from google import genai  # type: ignore
        except ImportError:
            print("[GeminiBackend] paquet 'google-genai' absent -- pip install google-genai. Degradation: None.", file=sys.stderr)
            return None
        api_key = os.environ.get("GEMINI_API_KEY")
        if not api_key:
            print("[GeminiBackend] GEMINI_API_KEY absent de l'environnement. Degradation: None.", file=sys.stderr)
            return None
        client = genai.Client(api_key=api_key)
        return cls(client=client, model=model)

    def choose_operator(self, fact: ReferenceFact) -> OperatorChoice:
        prompt = build_prompt(fact)
        response = self.client.models.generate_content(model=self.model, contents=prompt)
        text = response.text or ""
        return _parse_json_choice(text)


# ---------------------------------------------------------------------------
# Backend factice deterministe -- AUCUN reseau. Le seul backend utilisable
# dans ce sandbox. Deux modes de construction :
#   - `fixed_operator`: repond toujours le meme operateur (utile pour un
#     test d'accord parfait, ou pour simuler un backend "toujours KNOWS").
#   - `answers`: dict fact.id -> operateur, pour simuler un desaccord
#     precis et connu sur des faits particuliers (le patron utilise par
#     le test `test_forced_disagreement_is_detected`).
# Par defaut (aucun des deux fourni), reproduit `expected_operator` de
# chaque ReferenceFact -- utile comme "backend de reference parfait".
# ---------------------------------------------------------------------------

@dataclass
class DeterministicStubBackend(OperatorChoiceBackend):
    name: str
    fixed_operator: Optional[str] = None
    answers: Optional[dict[str, str]] = None

    def choose_operator(self, fact: ReferenceFact) -> OperatorChoice:
        if self.fixed_operator is not None:
            op = self.fixed_operator
        elif self.answers is not None and fact.id in self.answers:
            op = self.answers[fact.id]
        else:
            op = fact.expected_operator
        return OperatorChoice(
            operator=op,
            justification=f"[stub deterministe {self.name}] reponse fixee d'avance pour {fact.id}",
            raw_response=json.dumps({"operator": op, "justification": "stub"}),
        )


# ---------------------------------------------------------------------------
# Collecte -- interroge chaque backend sur chaque fait. Purement
# sequentiel et synchrone (pas de parallelisme reseau) -- ce script est un
# outil de mesure ponctuelle, pas un service ; simplicite avant
# performance.
# ---------------------------------------------------------------------------

def collect_responses(
    backends: list[OperatorChoiceBackend],
    facts: list[ReferenceFact],
) -> dict[str, dict[str, OperatorChoice]]:
    """Retourne {fact_id: {backend_name: OperatorChoice}}."""
    results: dict[str, dict[str, OperatorChoice]] = {}
    for fact in facts:
        results[fact.id] = {}
        for backend in backends:
            results[fact.id][backend.name] = backend.choose_operator(fact)
    return results


# ---------------------------------------------------------------------------
# Scoring d'accord inter-annotateurs.
#
# raw_agreement_per_fact : pour un fait donne, fraction des paires de
# backends (i, j) qui ont choisi le meme operateur (1.0 si tous d'accord,
# 0.0 si tous differents deux a deux).
#
# cohens_kappa : pour EXACTEMENT deux annotateurs -- formule standard
# (Cohen 1960) : kappa = (p_o - p_e) / (1 - p_e), avec p_o l'accord
# observe et p_e l'accord attendu par hasard sous la distribution
# marginale de chaque annotateur.
#
# fleiss_kappa : pour N >= 2 annotateurs (generalisation, Fleiss 1971),
# utilisee par defaut des que plus de deux backends sont fournis --
# fonctionne aussi pour N=2 mais Cohen's kappa est rapporte separement
# dans ce cas car c'est la formule explicitement demandee.
# ---------------------------------------------------------------------------

def raw_agreement_per_fact(choices: dict[str, OperatorChoice]) -> float:
    ops = [c.operator for c in choices.values()]
    if len(ops) < 2:
        return 1.0
    pairs = list(itertools.combinations(ops, 2))
    agree = sum(1 for a, b in pairs if a == b)
    return agree / len(pairs)


def overall_raw_agreement(results: dict[str, dict[str, OperatorChoice]]) -> float:
    per_fact = [raw_agreement_per_fact(choices) for choices in results.values()]
    return sum(per_fact) / len(per_fact) if per_fact else 0.0


def cohens_kappa(results: dict[str, dict[str, OperatorChoice]], backend_a: str, backend_b: str) -> float:
    """Cohen's kappa entre exactement deux backends, agrege sur tous les
    faits. Retourne 1.0 si accord parfait, 0.0 si accord = hasard, peut
    etre negatif si accord pire que le hasard. Retourne 1.0 par convention
    si les deux annotateurs n'ont chacun qu'une seule categorie identique
    sur l'ensemble des faits (accord parfait, p_e = 1, evite une division
    par zero qui casserait le rapport sur un jeu de faits trop homogene)."""
    labels_a = [results[fid][backend_a].operator for fid in results]
    labels_b = [results[fid][backend_b].operator for fid in results]
    n = len(labels_a)
    if n == 0:
        return 0.0

    p_o = sum(1 for a, b in zip(labels_a, labels_b) if a == b) / n

    categories = sorted(set(labels_a) | set(labels_b))
    p_e = 0.0
    for cat in categories:
        p_a = labels_a.count(cat) / n
        p_b = labels_b.count(cat) / n
        p_e += p_a * p_b

    if p_e >= 1.0:
        return 1.0 if p_o >= 1.0 else 0.0
    return (p_o - p_e) / (1 - p_e)


def fleiss_kappa(results: dict[str, dict[str, OperatorChoice]]) -> float:
    """Fleiss' kappa (1971) pour N >= 2 annotateurs. Chaque fait est un
    "sujet", chaque backend un "annotateur", vote unique par sujet
    (n_ij in {0,1} ici puisque chaque backend donne exactement une
    reponse par fait)."""
    fact_ids = list(results.keys())
    n_facts = len(fact_ids)
    if n_facts == 0:
        return 0.0
    n_raters = len(next(iter(results.values())))
    if n_raters < 2:
        return 1.0

    categories = sorted({c.operator for choices in results.values() for c in choices.values()})
    cat_index = {c: i for i, c in enumerate(categories)}

    # n_ij : nombre d'annotateurs ayant assigne la categorie j au sujet i
    counts = [[0] * len(categories) for _ in range(n_facts)]
    for i, fid in enumerate(fact_ids):
        for choice in results[fid].values():
            counts[i][cat_index[choice.operator]] += 1

    p_j = [sum(counts[i][j] for i in range(n_facts)) / (n_facts * n_raters) for j in range(len(categories))]

    p_i = []
    for i in range(n_facts):
        s = sum(c * c for c in counts[i])
        p_i.append((s - n_raters) / (n_raters * (n_raters - 1)))

    p_bar = sum(p_i) / n_facts
    p_e_bar = sum(p * p for p in p_j)

    if p_e_bar >= 1.0:
        return 1.0 if p_bar >= 1.0 else 0.0
    return (p_bar - p_e_bar) / (1 - p_e_bar)


@dataclass
class AgreementReport:
    overall_raw_agreement: float
    kappa_label: str
    kappa_value: float
    per_fact_agreement: list[tuple[str, str, float]]  # (fact_id, fact_text, agreement)
    n_backends: int
    n_facts: int

    def most_controversial(self, top_n: int = 5) -> list[tuple[str, str, float]]:
        return sorted(self.per_fact_agreement, key=lambda t: t[2])[:top_n]

    def render(self) -> str:
        lines = [
            "=== Rapport d'accord inter-operateurs CSTL ===",
            f"Backends: {self.n_backends}  |  Faits: {self.n_facts}",
            f"Accord brut global (moyenne des paires par fait): {self.overall_raw_agreement:.1%}",
            f"{self.kappa_label}: {self.kappa_value:.3f}",
            "",
            "Faits les plus controverses (accord le plus faible en tete):",
        ]
        for fid, text, agreement in self.most_controversial():
            lines.append(f"  [{agreement:.0%}] {fid}: {text}")
        return "\n".join(lines)


def build_report(results: dict[str, dict[str, OperatorChoice]], facts: list[ReferenceFact]) -> AgreementReport:
    fact_by_id = {f.id: f for f in facts}
    per_fact = [
        (fid, fact_by_id[fid].text, raw_agreement_per_fact(choices))
        for fid, choices in results.items()
    ]
    backend_names = sorted(next(iter(results.values())).keys()) if results else []

    if len(backend_names) == 2:
        kappa_label = f"Cohen's kappa ({backend_names[0]} vs {backend_names[1]})"
        kappa_value = cohens_kappa(results, backend_names[0], backend_names[1])
    else:
        kappa_label = f"Fleiss' kappa ({len(backend_names)} annotateurs)"
        kappa_value = fleiss_kappa(results)

    return AgreementReport(
        overall_raw_agreement=overall_raw_agreement(results),
        kappa_label=kappa_label,
        kappa_value=kappa_value,
        per_fact_agreement=per_fact,
        n_backends=len(backend_names),
        n_facts=len(facts),
    )


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

BACKEND_REGISTRY: dict[str, "callable"] = {
    # backends "stub" preconfigures pour --dry-run / demonstrations locales
    "stub_a": lambda: DeterministicStubBackend(name="stub_a"),
    "stub_b": lambda: DeterministicStubBackend(name="stub_b"),
    "anthropic": lambda: AnthropicBackend.from_env(),
    "gemini": lambda: GeminiBackend.from_env(),
}


def resolve_backends(names: list[str]) -> list[OperatorChoiceBackend]:
    resolved = []
    for name in names:
        factory = BACKEND_REGISTRY.get(name)
        if factory is None:
            print(f"[operator_agreement_study] backend inconnu: {name!r} -- ignore. "
                  f"Backends disponibles: {sorted(BACKEND_REGISTRY)}", file=sys.stderr)
            continue
        backend = factory()
        if backend is None:
            print(f"[operator_agreement_study] backend {name!r} indisponible dans cet "
                  f"environnement (degradation propre, voir stderr ci-dessus) -- ignore.", file=sys.stderr)
            continue
        resolved.append(backend)
    return resolved


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backends", default="stub_a,stub_b",
                     help="Liste separee par des virgules parmi: " + ", ".join(sorted(BACKEND_REGISTRY)))
    ap.add_argument("--dry-run", action="store_true",
                     help="Force l'usage des backends stub_a,stub_b (aucun reseau) -- equivalent a --backends stub_a,stub_b.")
    ap.add_argument("--json-out", type=Path, default=None,
                     help="Ecrit les reponses brutes (par fait, par backend) dans ce fichier JSON.")
    args = ap.parse_args()

    backend_names = ["stub_a", "stub_b"] if args.dry_run else args.backends.split(",")
    backends = resolve_backends(backend_names)

    if len(backends) < 2:
        print("[operator_agreement_study] au moins 2 backends operationnels sont requis pour "
              "mesurer un accord inter-annotateurs. Backends resolus: "
              f"{[b.name for b in backends]}. Arret.", file=sys.stderr)
        return 1

    print(f"[operator_agreement_study] backends actifs: {[b.name for b in backends]}")
    if any(isinstance(b, DeterministicStubBackend) for b in backends):
        print("[operator_agreement_study] AVERTISSEMENT: au moins un backend est un stub "
              "deterministe -- ceci N'EST PAS une mesure empirique reelle. Voir l'en-tete "
              "de ce fichier pour les instructions de configuration d'un vrai backend LLM.")

    results = collect_responses(backends, REFERENCE_FACTS)
    report = build_report(results, REFERENCE_FACTS)
    print()
    print(report.render())

    if args.json_out:
        serializable = {
            fid: {name: {"operator": c.operator, "justification": c.justification}
                  for name, c in choices.items()}
            for fid, choices in results.items()
        }
        args.json_out.write_text(json.dumps(serializable, ensure_ascii=False, indent=2))
        print(f"\n[operator_agreement_study] reponses brutes ecrites dans {args.json_out}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
