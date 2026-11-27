# CSTL Dashboard

Tableau de bord local (bibliotheque standard Python uniquement) pour l'OS
CSTL agentic kernel, a cote du vrai serveur Rust CSTL (`cargo run --release`,
port 5050 par defaut).

```
python3 dashboard/server.py
# puis http://127.0.0.1:5099/
```

Voir l'en-tete de `dashboard/server.py` pour le detail complet (variables
d'environnement, ce que chaque endpoint fait reellement, limites honnetes
de chaque integration -- Hermes/Ollama, OpenClaw, Telegram/Obsidian, PTY).
Ce fichier ne documente que la partie **Actions** (execution reelle
d'actions committees par le RestrictedCouncil).

## Actions : CSTL decide, le dashboard execute

Demande d'Olivier: que l'OS CSTL agentic puisse reellement AGIR --
executer des actions, pas seulement transporter/auditer du sens entre
agents -- en gardant le dashboard comme cockpit unique (pas un nouveau
runtime separe) et CSTL comme couche de DECISION/AUDIT/GOUVERNANCE.

### Flux complet

```
1. PROPOSER   un agent (LLM, humain, script) construit un payload
              purpose=action_request avec tool= et command= dans
              INTENT_PAYLOAD, et l'envoie au serveur CSTL (port 5050).
              -> onglet "Actions" du dashboard (POST /api/actions/propose),
                 ou n'importe quel client CSTL (sdk/python/cstl_client.py).
              -> stocke dans adn_store, committed=0 par defaut. NE
                 DECLENCHE AUCUNE EXECUTION a ce stade.

2. VOTER /    le RestrictedCouncil (quorum humain deja existant, voir
   COMMITTER  src/restricted_council.rs) doit committer ce hash via un
              vrai purpose=council_decision SIGNE Ed25519 par un membre
              enregistre (sdk/python/cstl_client.py::send_council_decision,
              ou le bot Telegram existant -- src/telegram_council.rs).
              -> le mecanisme de quorum/signature est GENERIQUE par hash:
                 aucun changement Rust n'a ete necessaire pour qu'il
                 committe un purpose=action_request, exactement comme
                 il committe n'importe quel autre purpose deja stocke
                 dans adn_store.
              -> adn_store.committed passe a 1 (verifiable en lisant
                 directement cstl_adn.db, colonne committed).

3. EXECUTER   dashboard/action_executor.py::run_action_poll_cycle() --
              appele automatiquement toutes les ~5s par un thread daemon
              dans dashboard/server.py -- lit le fichier ADN en LECTURE
              SEULE, trouve les action_request fraichement committees non
              encore executees (table locale dashboard_actions.db, separee
              du fichier ADN de CSTL), execute REELLEMENT la commande
              (`subprocess.run(["bash", "-lc", command], ...)`), et
              enregistre le resultat localement (hash, tool, command,
              exit_code, stdout/stderr tronques a 4000 caracteres,
              executed_at).

4. AUDITER    le resultat de l'execution est renvoye au serveur CSTL comme
              un NOUVEAU payload purpose=action_result
              (in_reply_to=<hash de la demande>, tool=, exit_code=) plus
              une RELATION [type=EXECUTED, subject=<tool>, object=<hash>]
              -- ferme la boucle: l'execution elle-meme devient un
              evenement CSTL trace dans audit_trail, pas seulement un
              log local.
```

`GET /api/actions` combine (lecture seule) toutes les entrees
`purpose=action_request` de l'adn_store -- committees OU EN ATTENTE de vote
-- avec leur statut d'execution local. `POST /api/actions/propose` ne fait
que l'etape 1 (proposer) : il ne committe rien.

### Limite honnete, assumee des le depart : `tool=` est INFORMATIF SEULEMENT

Dans cette premiere passe, le champ `tool` (`claude-code` / `openclaw` /
`hermes` / `shell`) est **loggue et stocke, mais n'affecte pas l'execution**
: TOUTE commande, quel que soit l'outil choisi, passe par le **meme shell
partage** (`bash -lc <command>`, `dashboard/action_executor.py::execute_action`),
jamais par une session PTY interactive specifique a l'outil.

**Ce qui resterait a faire pour un vrai routage par outil**, plutot qu'un
shell partage, vers une session PTY reelle (`dashboard/pty_bridge.py`,
deja utilise par les onglets Hermes/OpenClaw/Claude Code du dashboard pour
un terminal interactif via WebSocket) :
- Faire correspondre chaque `tool=` a une session PTY DEJA OUVERTE (ou en
  ouvrir une a la demande) plutot qu'a un nouveau `subprocess.run()` isole
  -- ce qui suppose de decider comment une commande "action_request" entre
  dans une session interactive existante (injecter du texte dans le PTY et
  detecter la fin de sortie n'est pas la meme chose qu'un appel
  `subprocess.run()` bloquant avec exit_code propre).
- Gerer le cas ou aucune session PTY pour cet outil n'est ouverte (lancer
  une session de zero ? refuser l'action ? file d'attente ?).
- Definir un protocole de fin de commande fiable dans un flux PTY partage
  (un vrai terminal n'a pas de notion native d'"exit_code d'une commande
  precise" au milieu d'un flux continu -- contrairement a
  `subprocess.run()`).
- Isolation : deux actions committees pour le meme outil en concurrence
  ne doivent pas se marcher dessus dans la meme session PTY.

Ce travail n'est **pas fait** dans cette v1 -- `execute_action()` ignore
deliberement `tool` pour l'execution elle-meme (seulement pour le
logging/l'audit), ce qui est honnete mais limite : pas d'isolation par
outil, pas de contexte de session persistant entre deux actions.

### Bases de donnees

- `cstl_adn.db` (racine du depot, ou `CSTL_ADN_DB_PATH`) : le VRAI fichier
  ADN de CSTL, proprietaire du serveur Rust. `action_executor.py` ne
  l'ouvre **jamais qu'en lecture seule** (`file:...?mode=ro`) -- jamais
  d'ecriture directe dedans depuis ce dashboard.
- `dashboard/dashboard_actions.db` (ou `CSTL_DASHBOARD_ACTIONS_DB`) : base
  SEPAREE, proprietaire du dashboard seul (table `executed_actions`) --
  sert uniquement a ne jamais re-executer deux fois la meme action
  committee.

### Verification

`dashboard/test_action_executor_e2e.py` lance un VRAI serveur CSTL compile
(`target/release/cstl_parser`) dans un repertoire jetable, enregistre un
vrai agent "Olivier" (seul membre autorise par defaut du RestrictedCouncil),
envoie un vrai `action_request`, le committe via un vrai `council_decision`
signe, appelle `run_action_poll_cycle()` pour de vrai, et confirme par
relecture directe des deux fichiers SQLite (adn_db + dashboard_actions.db)
que l'execution et l'audit ont reellement eu lieu -- aucune etape simulee.

```
cargo build --release
python3 dashboard/test_action_executor_e2e.py
```
