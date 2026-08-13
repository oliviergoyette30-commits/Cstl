"""
cstl_verify_public_kb.py — Couche 3a de l'architecture CSTL (v2, corrigée)
Vérification d'une relation CSTL (subject, predicate, object) contre Wikidata.

Corrections apportées après le test réel du 2026-08-06 :
- Recherche générique "any property" trop imprécise -> mapping predicate -> propriété Wikidata ciblée
- Désambiguïsation d'entité insuffisante -> exclusion des pages de désambiguïsation (Q4167410)
- Fallback générique conservé si le prédicat n'est pas dans le mapping connu

Usage :
    from cstl_verify_public_kb import verify_relation
    result = verify_relation("Marie Curie", "born_in", "Warsaw")
"""

import requests
import time

WIKIDATA_SEARCH_API = "https://www.wikidata.org/w/api.php"
WIKIDATA_SPARQL_ENDPOINT = "https://query.wikidata.org/sparql"

HEADERS = {
    "User-Agent": "CSTL-Verifier/1.1 (research project; contact: orchestrator)"
}

# Mapping prédicat CSTL -> propriété Wikidata (PID) connue.
# Étendre cette table au fur et à mesure des besoins réels du projet.
PREDICATE_TO_WIKIDATA_PROPERTY = {
    "born_in": "P19",       # place of birth
    "died_in": "P20",       # place of death
    "spouse": "P26",        # spouse
    "child_of": "P22",      # father / could also check P25 (mother)
    "employer": "P108",     # employer
    "occupation": "P106",   # occupation
    "nationality": "P27",   # country of citizenship
    "founder_of": "P112",   # founded by (inverse direction, handled specially)
    "located_in": "P131",   # located in administrative territorial entity
    "part_of": "P361",      # part of
    "author_of": "P50",     # author (inverse direction)
    "capital_of": "P36",    # capital
}

DISAMBIGUATION_PAGE_QID = "Q4167410"

# Propriétés pour lesquelles une chaîne transitive à plusieurs sauts a un
# sens sémantique réel (hiérarchies d'imbrication géographique/administrative).
# Ajoutée le 2026-08-12 après le cas Tour Eiffel : P131 direct
# (Q243 -> Q90) échoue car Wikidata modélise Q243 -> Q259463 (7e
# arrondissement) -> Q90, un niveau de granularité plus fin qu'un lien direct.
CHAINABLE_PROPERTIES = {"P131", "P361"}  # located in admin entity / part of


def _is_disambiguation_page(qid: str) -> bool:
    """Vérifie si un QID pointe vers une page de désambiguïsation (à exclure)."""
    query = f"""
    ASK {{ wd:{qid} wdt:P31 wd:{DISAMBIGUATION_PAGE_QID} . }}
    """
    try:
        resp = requests.get(
            WIKIDATA_SPARQL_ENDPOINT,
            params={"query": query, "format": "json"},
            headers=HEADERS,
            timeout=10,
        )
        resp.raise_for_status()
        return resp.json().get("boolean", False)
    except requests.RequestException:
        return False  # en cas de doute, ne pas bloquer sur ce filtre seul


def _search_entity(label: str, lang: str = "fr", max_candidates: int = 3):
    """Cherche une entité Wikidata, en excluant les pages de désambiguïsation.
    Retourne (qid, label_trouve) du premier candidat valide, ou (None, raison).

    Correction du 2026-08-10 : l'anglais est essayé EN PREMIER, indépendamment
    de `lang`. Observé empiriquement que la recherche anglaise résout mieux
    les entités communes (ex: "Warsaw" -> Q270, la ville, en 1ere position ;
    alors que la recherche via lang="fr" avait renvoyé Q106494408, un jeu
    video du meme nom, en 1ere position). Wikidata etant a dominante
    anglophone dans son indexation de recherche, l'anglais reste la langue
    la plus fiable pour la resolution initiale meme si les labels finaux
    restent bilingues (label_trouve conserve la langue trouvee).
    """
    languages_to_try = ["en"] + [l for l in (lang,) if l != "en"]
    for language in languages_to_try:
        params = {
            "action": "wbsearchentities",
            "search": label,
            "language": language,
            "format": "json",
            "limit": max_candidates,
        }
        try:
            resp = requests.get(WIKIDATA_SEARCH_API, params=params, headers=HEADERS, timeout=10)
            resp.raise_for_status()
            results = resp.json().get("search", [])
            for candidate in results:
                qid = candidate["id"]
                if not _is_disambiguation_page(qid):
                    return qid, candidate.get("label", label)
                time.sleep(0.2)
            if results:
                continue
        except requests.RequestException as e:
            return None, f"network_error: {e}"
    return None, "no_valid_non_disambiguation_entity_found"


def _query_specific_property(subject_qid: str, object_qid: str, property_id: str):
    """Vérifie si subject_qid a exactement property_id pointant vers object_qid
    (dans un sens ou l'autre, pour couvrir les relations encodées à l'envers)."""
    query = f"""
    ASK {{
      {{ wd:{subject_qid} wdt:{property_id} wd:{object_qid} . }}
      UNION
      {{ wd:{object_qid} wdt:{property_id} wd:{subject_qid} . }}
    }}
    """
    try:
        resp = requests.get(
            WIKIDATA_SPARQL_ENDPOINT,
            params={"query": query, "format": "json"},
            headers=HEADERS,
            timeout=15,
        )
        resp.raise_for_status()
        return resp.json().get("boolean", False), None
    except requests.RequestException as e:
        return None, f"network_error: {e}"


def _query_property_neighbors(qid: str, property_id: str, limit: int = 50):
    """QID directement reliés à qid par property_id, dans les deux sens.
    Utilisé pour l'expansion BFS de _find_property_chain."""
    query = f"""
    SELECT DISTINCT ?n WHERE {{
      {{ wd:{qid} wdt:{property_id} ?n . }}
      UNION
      {{ ?n wdt:{property_id} wd:{qid} . }}
    }}
    LIMIT {limit}
    """
    try:
        resp = requests.get(
            WIKIDATA_SPARQL_ENDPOINT,
            params={"query": query, "format": "json"},
            headers=HEADERS,
            timeout=15,
        )
        resp.raise_for_status()
        bindings = resp.json().get("results", {}).get("bindings", [])
        neighbors = []
        for b in bindings:
            neighbor_qid = b["n"]["value"].rsplit("/", 1)[-1]
            if neighbor_qid.startswith("Q"):
                neighbors.append(neighbor_qid)
        return neighbors
    except requests.RequestException:
        return []


def _find_property_chain(subject_qid: str, object_qid: str, property_id: str,
                          max_hops: int = 4, max_expansions: int = 40):
    """BFS jusqu'à max_hops sauts sur les arêtes property_id, cherchant un
    chemin subject_qid -> ... -> object_qid.

    Utilisé quand le lien direct (1 saut) échoue mais que la propriété est
    transitive par nature : une chaîne administrative tour -> arrondissement
    -> ville est un fait tout aussi réel qu'un lien direct tour -> ville,
    juste modélisé à un grain plus fin dans Wikidata.

    Deux caps indépendants protègent la recherche :
    - max_hops borne la PROFONDEUR (et empêche toute boucle infinie même en
      cas de cycle dans le graphe, via `visited`).
    - max_expansions borne le NOMBRE TOTAL de noeuds explorés (donc d'appels
      SPARQL), pour éviter l'explosion combinatoire quand un noeud a un fort
      facteur de branchement (ex: une grande ville a des milliers de choses
      qui pointent vers elle via P131) — sans ce cap, un graphe large mais
      peu profond pourrait quand même déclencher des dizaines de milliers
      d'appels avant d'atteindre max_hops.

    Retourne (chemin, exhausted) :
    - chemin : liste des sauts [(from_qid, to_qid), ...] si trouvé, sinon None.
    - exhausted=True  : la recherche a exploré tout ce qu'autorisait max_hops
      (queue vidée) -> "pas de chemin" est une conclusion sûre.
    - exhausted=False : max_expansions a été atteint avant la fin -> résultat
      NON CONCLUANT, à ne jamais traiter comme une preuve d'absence de chaîne.
    """
    if subject_qid == object_qid:
        return [], True

    from collections import deque

    visited = {subject_qid}
    queue = deque([(subject_qid, [])])
    expansions = 0

    while queue:
        if expansions >= max_expansions:
            return None, False  # budget épuisé : recherche interrompue, pas prouvé négatif

        current_qid, path = queue.popleft()
        if len(path) >= max_hops:
            continue

        expansions += 1
        for neighbor_qid in _query_property_neighbors(current_qid, property_id):
            if neighbor_qid == object_qid:
                return path + [(current_qid, neighbor_qid)], True
            if neighbor_qid not in visited:
                visited.add(neighbor_qid)
                queue.append((neighbor_qid, path + [(current_qid, neighbor_qid)]))
        time.sleep(0.1)  # ménager l'API Wikidata entre les expansions BFS

    return None, True  # queue vidée naturellement : recherche complète, pas de chemin


def _query_any_relation_exists(subject_qid: str, object_qid: str):
    """Fallback générique : n'importe quelle propriété directe reliant les deux entités."""
    query = f"""
    SELECT ?prop WHERE {{
      {{ wd:{subject_qid} ?prop wd:{object_qid} . }}
      UNION
      {{ wd:{object_qid} ?prop wd:{subject_qid} . }}
    }}
    LIMIT 5
    """
    try:
        resp = requests.get(
            WIKIDATA_SPARQL_ENDPOINT,
            params={"query": query, "format": "json"},
            headers=HEADERS,
            timeout=15,
        )
        resp.raise_for_status()
        bindings = resp.json().get("results", {}).get("bindings", [])
        if bindings:
            return True, bindings[0]["prop"]["value"]
        return False, None
    except requests.RequestException as e:
        return None, f"network_error: {e}"


def verify_relation(subject: str, predicate: str, obj: str, lang: str = "fr",
                     max_hops: int = 4, max_expansions: int = 40) -> dict:
    """Point d'entrée principal — couche 3a, version corrigée.

    1. Résout subject et object en QID, en excluant les pages de désambiguïsation
    2. Si le prédicat est dans le mapping connu -> vérifie CETTE propriété précise (P19, etc.)
       Si le lien direct échoue ET que la propriété est transitive
       (CHAINABLE_PROPERTIES) -> tente une chaîne à plusieurs sauts, bornée à
       la fois en profondeur (max_hops) et en nombre total de noeuds explorés
       (max_expansions), avant de conclure à l'échec.
    3. Sinon -> fallback générique (moins fiable, marqué comme tel dans le résultat)
    """
    subject_qid, subject_info = _search_entity(subject, lang)
    time.sleep(0.3)
    object_qid, object_info = _search_entity(obj, lang)

    if subject_qid is None or object_qid is None:
        return {
            "verified": "unchallenged_unproven",
            "source_url": None,
            "reason": "subject_or_object_not_resolved",
            "subject_resolution": subject_info,
            "object_resolution": object_info,
        }

    time.sleep(0.3)
    property_id = PREDICATE_TO_WIKIDATA_PROPERTY.get(predicate)
    chain = None

    if property_id:
        found, error = _query_specific_property(subject_qid, object_qid, property_id)
        check_method = f"targeted_property_{property_id}"

        if found is False and property_id in CHAINABLE_PROPERTIES:
            chain_hops, exhausted = _find_property_chain(
                subject_qid, object_qid, property_id,
                max_hops=max_hops, max_expansions=max_expansions,
            )
            if chain_hops:
                found = True
                error = None
                chain = chain_hops
                check_method = f"transitive_chain_{property_id}_{len(chain_hops)}_hops"
            elif not exhausted:
                # budget d'expansion épuisé avant la fin : on ne peut PAS
                # conclure à l'absence de chaîne, seulement que la recherche
                # bornée ne l'a pas trouvée. found reste False (donc la
                # relation ne sera pas confirmée) mais le check_method le dit
                # honnêtement plutôt que de prétendre à une recherche complète.
                check_method = f"transitive_chain_{property_id}_incomplete_expansion_budget_exceeded"
    else:
        found, error = _query_any_relation_exists(subject_qid, object_qid)
        check_method = "generic_fallback_any_property_less_reliable"

    if found is None:
        return {
            "verified": "unchallenged_unproven",
            "source_url": None,
            "reason": f"sparql_query_failed: {error}",
            "subject_qid": subject_qid,
            "object_qid": object_qid,
            "check_method": check_method,
            "chain": None,
            "property_id": property_id,
        }

    if found:
        return {
            "verified": "confirmed_external_source",
            "source_url": f"https://www.wikidata.org/wiki/{subject_qid}",
            "reason": f"property_confirmed_via_{check_method}",
            "subject_qid": subject_qid,
            "object_qid": object_qid,
            "check_method": check_method,
            "chain": chain,
            "property_id": property_id,
        }
    else:
        return {
            "verified": "unchallenged_unproven",
            "source_url": f"https://www.wikidata.org/wiki/{subject_qid}",
            "reason": f"entities_resolved_but_relation_not_confirmed_via_{check_method}",
            "subject_qid": subject_qid,
            "object_qid": object_qid,
            "check_method": check_method,
            "chain": None,
            "property_id": property_id,
        }


if __name__ == "__main__":
    import sys
    if len(sys.argv) == 4:
        s, p, o = sys.argv[1], sys.argv[2], sys.argv[3]
    else:
        s, p, o = "Marie Curie", "born_in", "Warsaw"
    print(f"Vérification : ({s}) {p} ({o})")
    result = verify_relation(s, p, o)
    for k, v in result.items():
        print(f"  {k}: {v}")
