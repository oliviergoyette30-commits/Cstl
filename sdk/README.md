# CSTL SDK Python

Client TCP minimal pour le serveur CSTL v5.0.0 (`src/server/`). Stdlib
Python uniquement (`socket`, `dataclasses`, `re`) -- aucune dependance
externe.

## Fichiers

- `python/cstl_client.py` -- `CstlClient`: construit et envoie un payload
  CSTL wire-format v5.0.0, parse la reponse du serveur.
- `python/cstl_orchestrator.py` -- CLI qui utilise `CstlClient` pour faire
  dialoguer deux parties (humain ou LLM) via le serveur, tour par tour.
- `python/cstl_llm_agent.py` -- **2026-09-04**: agent LLM reel (Ed25519 via
  `cryptography`, enregistrement dynamique via `purpose=agent_register`,
  messages signes). Voir sa section dediee plus bas -- ce fichier a une
  dependance externe (`cryptography`, `anthropic` optionnel) contrairement
  au reste de ce SDK.

## Ce que ce SDK fait reellement

- Construit un payload CSTL valide (`#!CSTL v5.0.0 MODE=A`, `META [...]`,
  `INTENT_PAYLOAD [...]`, `RELATION [...]`, `---END---`), avec citation
  automatique des valeurs contenant une virgule (sinon le parser serveur,
  `src/server/parser.rs`, les coupe silencieusement au split top-level).
- Envoie ce payload sur TCP au vrai serveur (`src/main.rs`, port 5050 par
  defaut, pas configurable cote serveur), avec une boucle de lecture qui
  accumule jusqu'a `---END---` -- miroir exact de
  `find_message_end` (`src/server/handler.rs`), pour ne pas supposer
  qu'une reponse tient dans un seul `recv()`.
- Parse la reponse reelle du serveur: `RELATION [type=received, ...]`,
  `VERIFICATION [...]` (Wikidata, `src/kb_verify.rs`),
  `CONSISTENCY [consistent=, contradictions=, cycles=, sigma=]`
  (`src/execution_lab.rs`), `SEMANTIC_WARNING [...]`,
  `AUDIT [hash=, parent_hash=, seq=]` (`src/server/audit.rs`).
- Distingue les 5 categories d'erreur reellement emises par
  `handler.rs`: `payload_too_large`, `security_rejected`,
  `validation_error`, `parse_error`, `error status=no_agent` -- via des
  exceptions Python typees (`CstlSecurityRejected`, `CstlParseError`,
  `CstlValidationError`, `CstlPayloadTooLarge`, `CstlNoAgentError`).
- Expose `send_council_decision()` et `send_detect_emergence()`, qui
  parlent aux deux purposes speciaux court-circuitant le pipeline normal
  (`council_decision`, `detect_emergence` dans `handler.rs`).

## Ce que ce SDK ne fait PAS

- **Aucun routage serveur-side agent A -> agent B.** Verifie dans le
  code: `AgentRegistry::route("communication")`
  (`src/agent_discovery.rs`, appele `handler.rs:470`) ne fait qu'un test
  d'existence -- il retourne le premier agent enregistre satisfaisant une
  capability, jamais un dispatch reel vers un second processus/LLM. Les
  agents "alice"/"bob" sont enregistres en dur au demarrage
  (`src/main.rs`), sans API pour en ajouter dynamiquement. Le mode
  `--relay` de `cstl_orchestrator.py` simule un dialogue A<->B en
  alternant COTE CLIENT quel `sender`/`receiver` est utilise a chaque
  tour -- ce n'est pas le serveur qui route, c'est l'orchestrateur.
- **Aucune notion de "converge/diverge/quarantine".** Ces termes
  apparaissent dans le diagramme conceptuel de `README.md` mais
  n'existent nulle part dans `src/server/handler.rs` ou ailleurs dans le
  code: aucune branche ne bloque ou ne redirige une reponse sur cette
  base. Les seuls signaux reels disponibles sont `SEMANTIC_WARNING`
  (jamais bloquant) et `CONSISTENCY` (`consistent`/`contradictions`/
  `cycles`/`sigma`, egalement jamais bloquant -- utilise seulement pour
  declencher une notification Telegram/Obsidian a destination d'un
  humain). Ce SDK n'ajoute aucune logique de classification au-dela de
  ces champs reels.
- **`cstl_client.py`/`cstl_orchestrator.py` eux-memes n'enregistrent
  aucun agent dynamiquement** -- ils parlent au registre statique
  (alice/bob codes en dur dans `main.rs`). L'enregistrement dynamique
  existe depuis le 2026-09-04 (`purpose=agent_register` cote serveur,
  `src/agent_discovery.rs::AgentRegistry` maintenant mutable) mais
  seulement `cstl_llm_agent.py` (ci-dessous) l'utilise dans ce SDK --
  parce qu'il exige une auto-signature Ed25519 et une dependance externe
  (`cryptography`) que le reste de ce SDK n'a delibrement pas.
- **Aucune configuration reseau cote serveur.** Port, adresse d'ecoute,
  chemin de la base SQLite et membre du RestrictedCouncil sont des
  constantes compile-time dans `src/main.rs`; le SDK ne fait
  qu'exposer `--host`/`--port` cote client.

## Verification

```bash
cargo build --release
cargo run --release &            # demarre le serveur sur 127.0.0.1:5050
python3 sdk/python/cstl_client.py --smoke-test
```

Le smoke-test envoie un payload valide, verifie `status=processed` et la
presence d'un `audit_hash`, envoie un payload invalide (sans `sender`) et
verifie qu'une `CstlValidationError` est levee, puis envoie une decision
de council par un expediteur non autorise et verifie
`purpose=council_decision_rejected, reason=not_authorized` -- preuve que
le SDK ne peut pas contourner le quorum humain reel
(`src/restricted_council.rs`).

Mode dialogue scripte:

```bash
python3 sdk/python/cstl_orchestrator.py --script agentA.txt agentB.txt
```

`agentA.txt`/`agentB.txt`: un message par ligne, envoyes en alternance
comme `agent_a`/`agent_b`, chaines via `parent_hash` (meme logique que
`send2.sh`/`send3.sh` a la racine du depot).

## `cstl_llm_agent.py` -- agent LLM signe (reecrit le 2026-11-17)

Dependances : `pip install cryptography` (signature Ed25519). Cerveaux :
Hermes via Ollama (aucun paquet requis : `CSTL_OLLAMA_HOST` / `CSTL_OLLAMA_PORT`
/ `CSTL_OLLAMA_MODEL`), Claude (`pip install anthropic`, modele
`claude-opus-5-5`, repli serveur automatique sur refus), Gemini
(`google-generativeai`, non revalide le 2026-11-17).

**Pourquoi la reecriture** : verifie en direct le 2026-11-17, l'agent ne
fonctionnait plus contre le serveur -- enregistrement rejete (champs requis
absents), messages en `parse_error` (virgule dans le texte), signature
calculee sur un autre contenu que celui envoye -- et rapportait quand meme
`status: sent`. Surtout, il n'envoyait que du texte libre : aucune des
verifications logiques de CSTL ne s'appliquait a ses messages.

```python
from cstl_llm_agent import CstlAgent, describe
agent = CstlAgent("my_agent", "anthropic")      # ou "hermes", "gemini"
agent.register()                                 # True si agent_register_ack
relation, resp = agent.tell("bob", "etat de la commande 42")
print(relation, describe(resp))
# ex. {'type': 'STATE', 'subject': 'order_42', 'object': 'shipped'} ACCEPTE (seq=3)
#     ... ACCEPTE AVEC SIGNALEMENTS: DEONTIC_AUDIT: 1 violation(s) avec l'historique
#     ... REJETE (operator_whitelist_rejected: E101 ...)
```

- `tell(receiver, subject)` : le modele formule UNE relation (JSON, operateur
  du catalogue officiel), l'agent l'envoie signee. Rien n'est envoye si le
  modele ne produit pas de relation exploitable.
- `send_relations(receiver, relations)` : envoi direct de relations.
- `resp.is_error` = rejet ; `resp.issues` = signalements sur un message
  accepte (contradiction de faits, contradiction deontique avec
  l'historique, avertissements semantiques). Les lire : CSTL n'en rejette
  qu'une partie.
- `send_message(receiver, texte)` : texte libre signe -- **aucune
  verification logique** ne s'y applique.

Dialogue autonome : `python3 sdk/python/cstl_llm_agent.py --name alice --provider hermes --turns 3`.

Verifie : `sdk/python/test_llm_agent_live.py` (en CI) fait tourner l'agent,
avec un modele simule, contre le vrai serveur : enregistrement, relation
acceptee, contradiction signalee, operateur rejete, contradiction visible
dans Obsidian et Graphify. **Non verifie ici** : une vraie generation par
Claude, Hermes ou Gemini (ni cle API ni Ollama dans cet environnement).

