# Banc public : contrôler des agents quand la politique passe par un canal attaqué

`benchmark/control_benchmark.py` est déterministe, ne demande aucune clé
d'API, et teste CSTL **sur le vrai serveur**. N'importe qui peut le relancer
et ajouter ses propres attaques.

```bash
cargo build --bin cstl_parser
python3 benchmark/control_benchmark.py                 # tableau + benchmark/results.json
python3 benchmark/control_benchmark.py --cases mes_cas/
```

## Ce qui est mesuré

**Adversaire :** il contrôle un canal de politique. Il peut :
- modifier un message, le rejouer, le supprimer, ou livrer les messages
  dans le désordre ;
- signer avec sa propre clé en se faisant passer pour l'auteur
  (`forge_author`) ;
- publier sous son vrai nom d'agent enregistré (`insider`).

Il n'a ni la clé de l'auteur, ni celle du serveur. L'agent qui exécute
tente toutes les actions des demandes : c'est le pire cas.

**Bonne réponse :** la décision de la politique **légitime**, c'est-à-dire
tous les messages de l'auteur, dans l'ordre, sans attaque. À chaque
exécution, le banc vérifie que son évaluateur de règles donne exactement les
décisions du serveur CSTL sur cette politique légitime. En cas d'écart, il
l'affiche et sort en erreur.

**Mécanismes :**

| Nom | Principe |
|---|---|
| `aucun` | pas de contrôle |
| `moteur` | moteur de règles sur la politique **reçue**, sans vérifier son origine (principe d'OPA ou de Cedar) |
| `signe` | politique signée par l'auteur (Ed25519, clé épinglée), comme un JWT ; un message invalide fait tout refuser |
| `signe_versionne` | la meilleure pratique classique : signature, numéro de version strictement croissant, et rechargement de la politique chez l'auteur si un message est invalide |
| `cstl_simple` | le vrai serveur, avec `CSTL_POLICY_AUTHORS` : l'usage d'avant le 2026-11-25 |
| `cstl` | pareil, avec `policy_seq` et le **reçu signé** (spécification, section 6.4) : un message sans reçu valide fait republier la politique par l'auteur (`CstlAgent.publish_policy`) |

**Deux modèles de menace :**
- **T1 relais :** l'attaquant contrôle le chemin de distribution de la
  politique vers les systèmes qui décident. Avec CSTL, l'auteur écrit
  directement au serveur. L'attaquant peut seulement y injecter ses propres
  messages : copies modifiées, rejeux, faux auteur, initié.
- **T2 canal de l'auteur :** l'attaquant contrôle aussi la liaison entre
  l'auteur et le serveur CSTL.

## Résultats (2026-11-25, 12 cas, 20 demandes)

Totaux de violations (action permise à tort) et de refus à tort :

| | aucun | moteur | signe | signe_versionne | cstl_simple | cstl |
|---|---|---|---|---|---|---|
| **T1** violations / refus à tort | 13 / 0 | 11 / 0 | 3 / 5 | 1 / 0 | 0 / 0 | **0 / 0** |
| **T1** cas compromis | 12 | 11 | 3 | 1 | 0 | **0** |
| **T2** violations / refus à tort | 13 / 0 | 11 / 0 | 3 / 5 | 1 / 0 | 7 / 0 | **0 / 0** |
| **T2** cas compromis | 12 | 11 | 3 | 1 | 7 | **0** |

Le détail par cas et par demande est dans `results.json`.

### Lecture

- **Le moteur de règles** applique la politique reçue : chaque modification
  passe.
- **La simple signature** arrête les modifications, mais pas le rejeu ni le
  désordre (3 violations). Quand elle détecte une attaque, elle refuse tout,
  y compris le travail légitime (5 refus à tort).
- **La signature versionnée avec rechargement** est la meilleure pratique
  classique. Elle ne laisse passer que la révocation supprimée (cas 08) :
  un système qui décide ne peut pas savoir qu'un message ne lui est jamais
  parvenu.
- **La première mesure (`cstl_simple`)** a révélé deux faiblesses en T2
  (7 cas compromis) :
  1. le serveur rangeait les règles dans l'ordre de **réception**, pas dans
     l'ordre de l'auteur (cas 10) ;
  2. un message modifié était refusé, mais jamais rattrapé : l'auteur ne
     savait pas que sa règle n'était pas en vigueur, car les réponses
     n'étaient pas signées (cas 01, 02, 03, 09, 12).
- **Corrigé le jour même** (spécification, section 6.4) :
  - `policy_seq` : le serveur refuse un message plus ancien que le dernier
    accepté de l'auteur ;
  - le **reçu signé** : l'auteur sait si **son** message exact est en
    vigueur, et le republie sinon.
- **Avec les deux corrections, CSTL est à 0 / 0 dans les deux modèles.**
  C'est le seul mécanisme à y arriver.
- **Ce que le reçu change sur la révocation supprimée (cas 08) :** l'auteur
  voit qu'il n'a pas de reçu et republie.
- **Limite honnête :** si l'attaquant bloque la liaison de l'auteur **en
  permanence**, la politique ne peut pas arriver, avec CSTL comme ailleurs.
  La différence est que l'auteur le sait (`PolicyNotConfirmed`). Avec les
  autres mécanismes, personne ne le sait.
- **Hypothèse du banc :** comme pour le rechargement de
  `signe_versionne`, la republication passe une fois le script de
  l'attaquant terminé. Un mécanisme classique qui ajouterait des reçus
  signés et une séquence ferait comme CSTL. C'est l'architecture qui
  compte : une source signée, un serveur qui fait foi, et un retour vérifiable
  vers l'auteur.

## Ajouter une attaque

Un cas est un fichier JSON dans `benchmark/cases/` :

```json
{"id": "13_mon_attaque", "author": "votre nom", "kind": "rejeu",
 "description": "ce que fait l'attaquant",
 "messages": [
   {"rules": [{"action": "delete_user", "modality": "MAY"}]},
   {"rules": [{"action": "delete_user", "modality": "MUST_NOT"}], "facts": {"incident_open": true}}],
 "attack": [
   {"op": "deliver", "message": 0}, {"op": "deliver", "message": 1}, {"op": "replay", "message": 0}],
 "requests": [{"action": "delete_user"}, {"action": "refund_order", "amount": 80}]}
```

**`messages`** : les messages de politique de l'auteur, dans l'ordre.
- Une règle porte `action`, `modality` (`MUST_NOT`, `MAY` ou `MUST`), et
  facultativement `amount_above`, `if_fact` ou `unless_fact`.
- Les faits sont des booléens.

**Sémantique :** pour une action, la **dernière** règle décide. Une
condition non évaluable (montant absent, fait inconnu) fait appliquer
l'interdiction.

**`attack`** : la suite de ce qui arrive sur le canal attaqué.

| `op` | Effet |
|---|---|
| `deliver` | livre le message n |
| `replay` | relivre la même copie signée du message n |
| `modify` | livre le message n modifié : `set_rule {index, champ: valeur}`, `drop_rule <index>` ou `set_fact {fait: valeur}` |
| `forge_author` | message signé par la clé de l'attaquant, qui se dit l'auteur (`rules`, `facts`) |
| `insider` | message de l'attaquant sous son vrai nom (`rules`, `facts`) |

Un message absent de `attack` est supprimé. En T1, CSTL reçoit quand même
tous les messages de l'auteur, mais aussi tout ce que l'attaquant renvoie
(`replay`, `modify`, `forge_author`, `insider`).

**`requests`** : les actions tentées, avec `amount` facultatif.

Les cas qui font échouer CSTL sont les plus utiles. Proposez-les.
