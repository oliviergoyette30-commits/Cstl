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
| `cstl` | le vrai serveur, avec `CSTL_POLICY_AUTHORS` |

**Deux modèles de menace :**
- **T1 relais :** l'attaquant contrôle le chemin de distribution de la
  politique vers les systèmes qui décident. Avec CSTL, l'auteur écrit
  directement au serveur. L'attaquant peut seulement y injecter ses propres
  messages : copies modifiées, rejeux, faux auteur, initié.
- **T2 canal de l'auteur :** l'attaquant contrôle aussi la liaison entre
  l'auteur et le serveur CSTL.

## Résultats (2026-11-25, 12 cas, 20 demandes)

Totaux de violations (action permise à tort) et de refus à tort :

| | aucun | moteur | signe | signe_versionne | cstl |
|---|---|---|---|---|---|
| **T1** violations / refus à tort | 13 / 0 | 11 / 0 | 3 / 5 | 1 / 0 | **0 / 0** |
| **T1** cas compromis | 12 | 11 | 3 | 1 | **0** |
| **T2** violations / refus à tort | 13 / 0 | 11 / 0 | 3 / 5 | **1 / 0** | 7 / 0 |
| **T2** cas compromis | 12 | 11 | 3 | **1** | 7 |

Le détail par cas et par demande est dans `results.json`.

### Lecture

- **Le moteur de règles** applique la politique reçue : chaque modification
  passe.
- **La simple signature** arrête les modifications. Elle laisse passer le
  rejeu et le désordre (3 violations). Quand elle détecte une attaque, elle
  refuse tout, y compris le travail légitime (5 refus à tort).
- **En T1, CSTL est le seul à 0 / 0.** La révocation supprimée (cas 08)
  trompe le meilleur mécanisme classique, mais pas CSTL : l'auteur écrit
  directement au serveur, qui fait foi.
- **En T2, CSTL est moins bon que `signe_versionne` (7 contre 1).** Les
  causes sont mesurées, cas par cas :
  1. **L'ordre :** CSTL range les règles dans l'ordre où il les **reçoit**,
     pas dans l'ordre de l'auteur. Deux messages authentiques livrés dans le
     désordre font gagner l'ancienne règle (cas 10).
  2. **Le rattrapage :** un message modifié est bien refusé (la signature ne
     correspond plus), mais la règle n'est alors jamais en vigueur, et
     l'auteur ne le sait pas (cas 01, 02, 03, 09, 12). Les réponses du
     serveur ne sont pas signées : sur ce canal, l'attaquant peut renvoyer
     un faux accusé de réception.
- **La révocation supprimée (cas 08)** n'est rattrapée par personne en T2.
  Aucun système ne sait qu'un message qu'il n'a jamais reçu existe.

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
