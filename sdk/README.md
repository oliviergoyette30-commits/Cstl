# CSTL SDK Python

Client TCP minimal pour le serveur CSTL v5.0.0 (`src/server/`). Stdlib
Python uniquement (`socket`, `dataclasses`, `re`) -- aucune dependance
externe.

## Fichiers

- `python/cstl_client.py` -- `CstlClient`: construit et envoie un payload
  CSTL wire-format v5.0.0, parse la reponse du serveur.
- `python/cstl_orchestrator.py` -- CLI qui utilise `CstlClient` pour faire
  dialoguer deux parties (humain ou LLM) via le serveur, tour par tour.

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
- **Aucune decouverte dynamique d'agents.** Le registre est fige a la
  compilation.
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
