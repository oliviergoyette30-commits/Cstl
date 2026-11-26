# CSTL délibération v6 — spécification (brouillon, 2026-11-26)

Plusieurs modèles délibèrent sur une question. Le moteur
(`sdk/python/cstl_deliberation.py`) tient l'état du débat, n'envoie à
chaque modèle que cet état, ferme chaque point en consensus ou en impasse,
et transforme les décisions en règles signées que la couche de contrôle
applique.

Cette section complète `CSTL_CONTROLE_SPEC_v1.md`. Elle n'en change rien :
- chaque réponse de modèle reste un message CSTL signé par le participant ;
- chaque réponse est vérifiée et auditée par le serveur ;
- la compilation passe par `publish_policy`, avec un reçu signé ;
- l'escalade passe par l'accord humain (section 7.5).

## 1. Points

- Toute proposition devient un **point** numéroté par le moteur : `p1`,
  `p2`, ...
- Un amendement de `p2` devient `p2.1`, puis `p2.1.1`. La profondeur est
  limitée à `max_depth` (2 par défaut), donc `p2.1.1.1` est refusé
  (`E_DEPTH`).
- Deux sortes de points :
  - **action** : `PERFORM` avec `modality=MUST|MUST_NOT|MAY` ;
  - **affirmation** : toute autre relation.
- Une proposition identique à un point existant n'en crée pas un nouveau.
  Elle compte comme un soutien à ce point.
- Les modèles désignent les points par leur numéro. Ils n'ont donc plus à
  reformuler les actions avec leurs propres mots, ce qui empêchait de
  comparer leurs positions (constaté le 2026-11-26 dans
  `tripartite/resultats`).

## 2. Ce que reçoit un modèle : l'état

À chaque tour, sans l'historique et sans règles recopiées :

```
#!CSTL v5.0.0 MODE=A
META [encoder=cstl_deliberation, produced_by=moderateur, round=2]
INTENT_PAYLOAD [purpose=deliberate, sender=moderateur, receiver=gemini, topic=lab_orders, round=2, max_rounds=6]
RELATION [type=point, subject=p1, object="ai_agent MAY order_routine_tests", supports=chatgpt:0.8;claude:0.7, rejects=gemini:0.6, why="gemini: signoff on every order"]
RELATION [type=point, subject=p2.1, object="ai_agent MAY order_tests_in_catalogue", amends=p2, supports=claude:0.7]
RELATION [type=closed, subject=p3, object=consensus, round=1]
RELATION [type=error, subject=gemini, object=E_SERVER, detail="operator_whitelist_rejected : E101 ..."]
---END---
```

Le message contient trois sortes de lignes :
- une ligne par point **ouvert**, avec toutes les positions. Seule la
  raison d'un refus est donnée, puisque c'est elle qu'il faut discuter ;
- les points fermés depuis le dernier tour de ce modèle ;
- ses erreurs à corriger, s'il y en a.

Au premier tour, une ligne `RELATION [type=ASK, ...]` porte la question.
Les instructions (le message système) sont données une fois par appel à une
API, ou une seule fois en mode manuel.

## 3. Ce que répond un modèle

Seulement des lignes `RELATION`, entre `#!CSTL v5.0.0 MODE=A` et
`---END---` :

```
RELATION [type=supports, subject=gemini, object=p1, sigma=0.8]
RELATION [type=rejects, subject=gemini, object=p2, sigma=0.7, reason="pourquoi"]
RELATION [type=abstains, subject=gemini, object=p3, sigma=0.5]
RELATION [type=PERFORM, subject=ai_agent, object=order_invasive_tests, modality=MUST_NOT, sigma=0.9]
RELATION [type=PERFORM, subject=ai_agent, object=order_tests_in_catalogue, modality=MAY, sigma=0.7, amends=p2]
```

**Vérifications locales.** Tout ce qui suit est refusé, avec son code :

| Code | Cause |
|---|---|
| `E_FORMAT` | Cadre `#!CSTL` … `---END---` absent. |
| `E_PROSE` | Une ligne qui n'est pas une `RELATION`, ou du texte après la fin. |
| `E_SIGMA` | `sigma` absent ou hors de 0 à 1. |
| `E_STANCE` | Une position dont le `subject` n'est pas le participant lui-même. |
| `E_POINT` | Une position sur un point inconnu. Une position sur un point déjà fermé est ignorée, sans erreur. |
| `E_NAME`, `E_MODALITY` | Identifiant ou modalité invalide. |
| `E_AMEND`, `E_DEPTH` | Amendement d'un point inconnu, ou trop profond. |
| `E_LIMIT` | Plus de `max_new_points` propositions dans le tour (2 par défaut). |

**Vérification par le serveur.** Les relations sont ensuite envoyées au
vrai serveur, signées par la clé du participant (`E_SERVER`) : opérateurs
officiels, contradictions, audit.

**Correction.** Une erreur est renvoyée au modèle dans l'état
(`type=error`), avec le détail exact. Il a `max_retries` essais (2 par
défaut).

**Participant défaillant.** Un tour sans réponse valide compte comme un
échec. Après `max_failures` échecs de suite (2 par défaut), le participant
est **exclu**, et les autres continuent.

## 4. Fermeture

À la fin de chaque tour, pour chaque point ouvert, on prend la dernière
position de chaque participant **actif**. Le proposant soutient son propre
point. Un soutien avec `sigma < min_confidence` (0,5 par défaut) compte
comme une abstention.

- **CONSENSUS** : tous les participants actifs soutiennent le point, et ils
  sont au moins `quorum` (par défaut la majorité des participants inscrits,
  et au moins 2). Depuis le 2026-11-26 : avant, un seul participant restant,
  les autres exclus, suffisait. Sans quorum, le point finit en DEADLOCK et
  part donc vers un humain.
- **DEADLOCK** : les positions n'ont pas bougé pendant `stall_rounds` tours
  (2 par défaut), et il reste un refus (`stalled_with_rejections`) ou une
  abstention (`stalled_without_quorum`).

Le débat s'arrête dès que l'une de ces conditions est vraie :
- plus aucun point n'est ouvert (`all_points_closed`) ;
- un tour entier n'a rien changé (`no_change_in_a_full_round`) ;
- il reste moins de `quorum` participants actifs
  (`not_enough_active_participants`) ;
- on a atteint `max_rounds` (6 par défaut).

Les points encore ouverts passent alors en DEADLOCK.

**Contre la « politesse structurée »** (des modèles qui s'approuvent et
ajoutent des idées sans fin) :
- au plus `max_new_points` propositions par tour ;
- au plus `max_points` points au total (20) ;
- la profondeur des amendements est limitée ;
- un tour sans changement arrête le débat.

**Pourquoi la fermeture repose sur les positions, et pas sur `sigma`.**
`sigma` est déclaré par le modèle lui-même. Le test du 2026-11-26 a montré
des modèles à 0,99 partout. Ici, `sigma` sert seulement de seuil : un
soutien timide ne compte pas.

## 5. État signé

Après chaque tour, l'état complet (points, positions, statuts) est publié
par le modérateur comme un arbre signé (`delib_<sujet>`). Le serveur
l'inscrit dans l'audit, et on peut le relire avec `fetch_tree`.

## 6. Exécution

`compile()` transforme les points d'**action** en `CONSENSUS` (ou approuvés
par un humain) en règles :

```
RELATION [type=PERFORM, subject=ai_agent, object=order_routine_tests, modality=MAY, reason="consensus deliberation lab_orders p1"]
```

Ces règles sont publiées par le modérateur avec `publish_policy`, et un
reçu signé confirme qu'elles sont en vigueur. Le modérateur doit être un
auteur de politique (`CSTL_POLICY_AUTHORS`). La couche de contrôle accorde
ou refuse ensuite les jetons selon ces règles.

- **Deux modalités différentes adoptées pour la même action** (par exemple
  `MAY` et `MUST_NOT`) : rien n'est compilé pour cette action.
- **Sans `CSTL_POLICY_AUTHORS` sur le serveur, le moteur refuse de
  démarrer.** Sinon, toute proposition d'un participant serait une règle
  active avant même d'être adoptée.
- **Les affirmations adoptées** restent dans l'état signé. Elles ne
  deviennent pas des règles.

## 7. Escalade humaine

`escalate()` demande, pour chaque point d'action en DEADLOCK, l'action
`deliberate.<sujet>.<point>`. Avec `CSTL_APPROVAL_ACTIONS=deliberate.*`, le
serveur répond `approval_required` et crée une demande d'accord.

L'humain décide avec `cstl-approve`. `resolve_escalations()` applique sa
décision :
- approuvé : le point passe en `resolved_by_human`, puis il est compilé avec
  la raison `human_decision` ;
- refusé : le point passe en `rejected_by_human`.

## 8. Mesures

- **Tokens.** Le moteur estime les tokens de chaque appel. Il les compare à
  une base « historique complet » : les mêmes tours si chaque modèle
  relisait toutes les réponses précédentes.
- **Mesuré avec des modèles déterministes** (2026-11-26) :
  - 2 tours et 8 points : 3 305 contre 3 452 (−4 %) ;
  - 8 tours et 40 points : 15 448 contre 44 615 (−65 %).
  - L'état grandit avec le nombre de points ouverts, l'historique avec la
    longueur du débat.
  - C'est une estimation prudente : la base ne compte pas les règles que le
    test manuel recopiait dans chaque message.
- **Le reste** (tours, essais, échecs, exclusions, statut de chaque point)
  est dans `summary()`.

## 9. Utilisation

```bash
CSTL_POLICY_AUTHORS=moderateur CSTL_APPROVERS="human_*" CSTL_APPROVAL_ACTIONS="deliberate.*" cargo run --bin cstl_parser
cstl-deliberate --topic lab_orders --question should_hospital_let_ai_order_lab_tests_alone \
    --model chatgpt=manual --model gemini=manual --model claude=manual \
    --server-key cstl_adn.db.server_key.pub --out deliberation.json
```

- `nom=manual` : le moteur affiche quoi coller dans le chatbot et lit la
  réponse.
- `nom=openrouter:openai/gpt-5-mini` : appel d'API.

En Python :

```python
d = Deliberation("lab_orders", "should_ai_order_lab_tests", ["chatgpt", "gemini", "claude"], models, server_key=key)
d.run(); d.compile(); d.escalate()
```

## 10. Limites

- **Le modérateur est un processus de confiance.** Il tient l'état et
  compile. Ses messages sont signés et l'état est audité, mais un modérateur
  malveillant pourrait mal compter les positions. Il faut une
  vérification indépendante de l'état publié (à faire).
- **Les affirmations ne sont pas vérifiées sur le fond.** La brique « preuves
  externes » n'est pas encore faite.
- **Rien n'est encore mesuré avec de vrais modèles.** Les chiffres ci-dessus
  viennent de modèles déterministes.
