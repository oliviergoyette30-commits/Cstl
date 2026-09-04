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

## `cstl_llm_agent.py` -- agent LLM reel signe (2026-09-04)

Dependances: `pip install cryptography` (obligatoire, signature Ed25519)
et `pip install anthropic` (optionnel -- sans lui, degradation propre,
voir plus bas).

Ce qui est **verifie en direct dans le sandbox de developpement** (donc
deja confirme fonctionnel, pas seulement structurellement):
- `cstl_signing_bytes()` (le port Python de
  `src/server/audit.rs::signing_bytes`) produit un hash BYTE-FOR-BYTE
  identique a l'implementation Rust sur une fixture croisee
  (`examples/print_signing_bytes_fixture.rs` cote Rust, meme fixture
  codee en dur dans `_structural_selftest()` cote Python).
- `register_agent()` et `send_signed_relation()` fonctionnent contre un
  **vrai serveur Rust demarre** (`cargo run --release`): enregistrement
  dynamique accepte (`agent_register_ack`), un message non signe du meme
  expediteur maintenant enregistre est rejete
  (`missing_signature_for_registered_agent`), le meme message
  correctement signe est accepte (`status=processed`).
- `AnthropicAgentBrain.from_env()` degrade proprement (`None`) dans cet
  environnement precis (ni paquet `anthropic`, ni `ANTHROPIC_API_KEY`).

Ce qui **n'a pas pu etre verifie ici** et reste a confirmer par
l'utilisateur sur sa propre machine: une vraie reponse generee par un
modele Claude, dans une conversation reelle avec le serveur CSTL.

```bash
pip install anthropic
export ANTHROPIC_API_KEY=sk-...
cargo run --release &                                    # serveur CSTL, port 5050
python3 sdk/python/cstl_llm_agent.py --peer-mode stdin --turns 4
```

Confirmer que: (1) `[LlmAgent] agent_register -> purpose='agent_register_ack'`
apparait, (2) chaque tour affiche une `relation` generee par le modele
(pas une erreur JSON) et `status='processed'`, (3) taper une reponse au
prompt `[Pair humain]` relance bien un tour suivant.

Auto-test structurel seul (aucun reseau, verifie ce qui EST verifiable
sans `anthropic`/cle -- c'est celui execute dans le sandbox de
developpement):

```bash
python3 sdk/python/cstl_llm_agent.py --structural-selftest
```
