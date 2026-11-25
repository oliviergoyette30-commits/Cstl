# CSTL contrôle — spécification v1 (figée le 2026-11-25)

Ce document fixe ce qu'il faut pour **écrire un client ou un serveur
compatible** avec la couche de contrôle de CSTL : identité, signature,
autorisation avant d'agir, jeton signé, politique, audit. Il ne couvre pas
les formats de données (arbre indenté, CSTL strict…) : voir
`docs/CSTL_ARBRE_INDENTE_SPEC.md` et `AGENTS.md`.

Les mots **DOIT**, **NE DOIT PAS** et **PEUT** ont leur sens normatif.

Preuves que ce texte suffit :
- `conformance/vectors.json` : entrées et sorties exactes. Python
  (`sdk/python/test_conformance.py`) et Rust
  (`tests/conformance_vectors_test.rs`) trouvent les mêmes octets et les
  mêmes verdicts.
- `conformance/run_server_conformance.py` : un testeur de serveur écrit à
  partir de ce texte seul, sans le SDK. Le serveur de ce dépôt passe 18/18.

En cas de désaccord entre ce texte et ces tests, c'est un bug à signaler.

## 1. Transport

- **TCP.** Le client envoie un message ; le serveur répond par un message.
  Chaque message se termine par une ligne `---END---`. Une connexion PEUT
  porter plusieurs messages à la suite.
- **JSON (facultatif).** `POST /v1/message` sur le port REST : le même
  message en JSON (section 3.4), la réponse en JSON. Le traitement est le
  même qu'en TCP.
- Le texte est en UTF-8.

## 2. Message

```
#!CSTL v5.0.0 MODE=A
META [encoder=a1, produced_by=a1, public_key=<64 hex>]
INTENT_PAYLOAD [purpose=action_request, sender=a1, receiver=server, action=wipe_db, request_id=r1, signature=<128 hex>]
RELATION [type=PERFORM, subject=a1, object=x, modality=MUST_NOT]
---END---
```

- **Première ligne :** `#!CSTL v<chiffres>.<chiffres>[.<chiffres>] MODE=A`.
  Les modes `B` et `C` sont réservés.
- **Ensuite, un bloc par ligne :** `NOM [cle=valeur, cle=valeur, ...]`.
  - `META` : exactement un. Champs obligatoires : `encoder`, `produced_by`.
  - `INTENT_PAYLOAD` : exactement un. Champs obligatoires : `purpose`,
    `sender`, `receiver`.
  - `RELATION` : zéro ou plus.
- **Fin :** une ligne `---END---`.

Un message sans un champ obligatoire est refusé : `META [status=error]`,
`purpose=validation_error`, et rien n'est audité.

## 3. Clés et valeurs

### 3.1 Clé

Non vide, sans espace, sans `=` `,` `[` `]` `"`.

### 3.2 Valeur

- Une valeur est une chaîne.
- Elle NE DOIT PAS contenir de `"`, de retour à la ligne, ni la suite
  `---END---`.
- Elle NE DOIT PAS commencer ni finir par un espace : le lecteur les retire.
- Elle DOIT être écrite entre guillemets si elle contient `,`, `[` ou `]`.
  Exemple : `reason="production, jamais"`. Les guillemets ne font pas partie
  de la valeur.

### 3.3 Lecture

- Couper le contenu entre crochets aux virgules hors guillemets.
- Couper chaque morceau au premier `=`.
- Retirer les espaces autour de la clé et de la valeur, puis les guillemets
  autour de la valeur.

### 3.4 Forme JSON

```json
{"version": "v5.0.0", "mode": "A",
 "meta": {"encoder": "a1", "produced_by": "a1", "public_key": "..."},
 "intent": {"purpose": "...", "sender": "a1", "receiver": "server", "signature": "..."},
 "relations": [{"type": "PERFORM", "subject": "a1", "object": "x"}]}
```

- Toutes les valeurs DOIVENT être des chaînes JSON.
- Une valeur que la section 3.2 interdit DOIT être refusée (HTTP 400), pas
  modifiée.
- Un champ de premier niveau inconnu DOIT être refusé.

## 4. Signature des messages

- **Algorithme :** Ed25519.
- **Clé publique :** `META.public_key`, 32 octets en hexadécimal minuscule
  (64 caractères).
- **Signature :** `INTENT_PAYLOAD.signature`, 64 octets en hexadécimal
  (128 caractères).

La signature porte sur les **octets canoniques** du message lu, pas sur
son texte. Le même message en texte ou en JSON a donc la même signature.

Construction, en UTF-8, chaque chaîne normalisée en **NFC** :

```
VERSION|<version>
MODE|<mode>
META|k1=v1|k2=v2...          (clés triées, sans PARENT_HASH)
INTENT|k1=v1|k2=v2...        (clés triées, sans signature ni rotation_signature)
RELATIONS|r1|r2...           (voir ci-dessous)
```

- Les lignes sont séparées par `\n`, sans `\n` final.
- Le tri se fait par point de code.
- Chaque relation s'écrit `k1=v1,k2=v2`, clés triées. Les relations ainsi
  écrites sont triées entre elles, puis jointes par `|`.
- Sans relation, la dernière ligne est `RELATIONS`.

Vecteurs : `conformance/vectors.json`, section `signing` (base, relations
triées, NFC, exclusions).

## 5. Identité

### 5.1 Enregistrement

```
INTENT_PAYLOAD [purpose=agent_register, sender=<nom>, receiver=server, name=<nom>, capabilities=communication, signature=...]
```

- Le message est signé avec la clé de l'agent (auto-signature).
- Réponse : `INTENT_PAYLOAD [purpose=agent_register_ack, name=<nom>]`.

### 5.2 Après l'enregistrement

Tout message dont le `sender` est cet agent DOIT être signé par **sa** clé
enregistrée. Sinon le serveur répond `purpose=signature_rejected`, avec une
`reason` et un `detail`, dont :

| `reason` | Cas |
|---|---|
| `missing_signature_for_registered_agent` | message non signé |
| `public_key_mismatch` | autre clé |
| `signature_invalid` | signature fausse ou message modifié après la signature (`detail=verification_failed`) |
| `sender_not_registered_strict_mode` | clé valide mais agent jamais enregistré (`CSTL_REQUIRE_SIGNATURES` strict) |

### 5.3 Valeurs par défaut

- Les signatures sont obligatoires par défaut.
- Le mode développement (`CSTL_DEV_MODE=1`, écoute locale seulement) admet
  des agents de démonstration sans clé.

### 5.4 Changement de clé

Un nouvel `agent_register` avec une autre clé DOIT porter en plus
`rotation_signature`, faite par l'**ancienne** clé sur les mêmes octets
canoniques. Sinon il est refusé.

### 5.5 Révocation d'une clé (ajouté le 2026-11-25)

```
INTENT_PAYLOAD [purpose=key_revoke, sender=<qui>, receiver=server, agent=<nom>, public_key=<hex>, reason=..., policy_seq=<n>, signature=...]
```

- `agent` vaut par défaut l'expéditeur ; `public_key` vaut par défaut la
  clé enregistrée de `agent`.
- Accepté si l'expéditeur prouve son identité (section 5.2), porte
  `policy_seq` (section 6.4), et :
  - c'est l'agent lui-même, et `public_key` est sa clé actuelle ;
  - ou c'est un auteur de politique (`CSTL_POLICY_AUTHORS`), pour n'importe
    quel agent.
- **Effet, définitif :** un message dont `META.public_key` est révoquée, ou
  dont l'expéditeur a une clé enregistrée révoquée, est refusé
  (`signature_rejected`, `reason=key_revoked`), y compris `agent_register`.
  Une règle, un fait, un rôle, un schéma ou un engagement d'arbre signé par
  cette clé ne compte plus, **même s'il a été écrit avant la révocation** :
  on ne sait pas depuis quand la clé est compromise.
- **Retour de l'agent :** la preuve de rotation (section 5.4) par une clé
  révoquée ne vaut rien, puisqu'un voleur l'a aussi. Un auteur de
  politique inscrit la nouvelle clé :
  `INTENT_PAYLOAD [purpose=key_enroll, agent=<nom>, public_key=<nouvelle>, policy_seq=<n>, ...]`.
  Le prochain `agent_register` de ce nom avec exactement cette clé passe,
  une seule fois. Sinon `agent_register_rejected`,
  `reason=key_revoked_enrollment_required`. Une clé révoquée ne peut pas
  être inscrite.
- **Réponse :** `purpose=key_revoked` (avec `agent`, `public_key`,
  `already_revoked`) ou `purpose=key_enrolled`, suivie d'un bloc `AUDIT`.
  Un refus : `purpose=control_denied, request=<purpose>, reason=...`
  (`signature_required`, `policy_seq_required`, `not_authorized`,
  `unknown_agent`, `invalid_agent`, `invalid_public_key`, `key_revoked`).
- **Limite :** un jeton déjà délivré à la clé révoquée reste valable
  jusqu'à son expiration (`CSTL_AUTHZ_TTL_SECS`, 60 s par défaut).

### 5.6 Identité d'entreprise OIDC (ajouté le 2026-11-25)

Avec `CSTL_OIDC_ISSUER`, `agent_register` DOIT porter `oidc_token=<JWT>`.
Le serveur vérifie, dans cet ordre :
- la signature RS256 ou ES256 (P-256), par la clé `kid` du JWKS
  (`CSTL_OIDC_JWKS_PATH` ou `CSTL_OIDC_JWKS_URL`). `none`, HS256 et un
  couple algorithme/clé incohérent sont refusés ;
- `iss` égal à `CSTL_OIDC_ISSUER` ;
- `aud` contenant `CSTL_OIDC_AUDIENCE`, s'il est configuré ;
- `exp` et `nbf`, avec 60 s de tolérance ;
- la revendication `CSTL_OIDC_NAME_CLAIM` (défaut `sub`) égale au nom de
  l'agent.

Sinon : `agent_register_rejected`, `reason=oidc_token_required`,
`oidc_signature_invalid`, `oidc_wrong_issuer`, `oidc_wrong_audience`,
`oidc_token_expired`, `oidc_name_mismatch`, `oidc_algorithm_refused`,
`oidc_key_unknown`, etc.

### 5.7 Identité de charge de travail SPIFFE (ajouté le 2026-11-25)

En TLS mutuel, avec `CSTL_SPIFFE_TRUST_DOMAIN=<domaine>`, l'identifiant
`spiffe://<domaine>/.../<nom>` du certificat client (SAN URI) fixe le nom de
la connexion. Tout message DOIT avoir `sender=<nom>`, sinon
`signature_rejected` avec `reason=spiffe_identity_mismatch`. Un identifiant
d'un autre domaine donne `spiffe_foreign_identity`. Avec
`CSTL_SPIFFE_REQUIRED=true`, l'absence d'identifiant donne
`spiffe_identity_required`. `token_status` en est exempté.

### 5.8 Fédération entre serveurs (ajouté le 2026-11-25)

- **Attestation.** Un agent demande au serveur qui le connaît
  (`CSTL_SERVER_NAME`, par exemple `acme`) :
  `purpose=identity_assertion`, signé. Réponse :
  `purpose=identity_assertion_issued` avec `issuer`, `agent`, `public_key`,
  `issued_at`, `expires_at` et `signature`. La signature est faite par la
  clé du serveur, sur :

  ```
  cstl-federation-v1
  issuer=<issuer>
  agent=<agent>
  public_key=<clé enregistrée, hex minuscule>
  issued_at=<s>
  expires_at=<s>
  ```

  La validité est `CSTL_FEDERATION_TTL_SECS`, 3600 par défaut.
- **Enregistrement chez le pair.** Un serveur pair qui fait confiance à
  `acme` (`CSTL_FEDERATION_PEERS=acme:<clé publique hex>`) enregistre
  `agent_register` sous le nom `acme.<agent>`. Le message porte
  `federation_issuer`, `federation_agent`, `federation_public_key`,
  `federation_issued_at`, `federation_expires_at` et
  `federation_signature`, et il est signé par **cette** clé.
- **Refus :** `federation_unknown_peer`, `federation_signature_invalid`,
  `federation_name_mismatch`, `federation_key_mismatch`,
  `federation_expired`. Un agent local ne peut pas prendre un nom
  `acme.*` (`federation_name_reserved`).
- **Expiration.** Une fois l'attestation expirée, les messages de
  `acme.<agent>` sont refusés (`signature_rejected`,
  `reason=federation_expired`) jusqu'à un nouvel enregistrement. Une clé
  révoquée chez l'émetteur n'obtient plus d'attestation : la révocation
  atteint le pair dans ce délai.

## 6. Règles

### 6.1 Forme

```
RELATION [type=PERFORM, subject=<agent>, object=<action>, modality=MUST_NOT|MUST|MAY, reason="..."]
```

- `MUST_NOT` interdit l'action à l'agent.
- `MUST` l'impose.
- `MAY` la permet.

### 6.2 Conditions vérifiées par le serveur

Le détail est dans `AGENTS.md`, section « Conditions the server
enforces » :
- `subject=role:<nom>` ;
- `valid_from` / `valid_until` ;
- `amount_above` / `amount_at_most` ;
- `if_fact` / `unless_fact` ;
- `priority`.

**Prudence :** une condition que le serveur ne peut pas évaluer (montant
absent, fait inconnu) DOIT faire appliquer une interdiction, et NE DOIT PAS
faire appliquer une obligation ni une permission.

### 6.3 Autorité

- Sans `CSTL_POLICY_AUTHORS`, la règle de n'importe quel agent enregistré
  compte, et une interdiction ne peut pas être levée.
- Avec cette variable, seules les règles signées par ces auteurs comptent ;
  la plus prioritaire, puis la plus récente, décide.

### 6.3 bis Actions gouvernées (ajouté le 2026-11-25)

Un serveur PEUT être installé avec une liste d'actions **gouvernées**
(`CSTL_GOVERNED_ACTIONS`, noms exacts ou `préfixe*`, séparés par des
virgules). La liste est fixée à l'installation, jamais par la politique.

- Une action gouvernée n'est permise que si un auteur de politique a écrit
  **au moins une règle** pour elle (pour cet agent), qu'elle s'applique ou
  non. S'il en existe, la décision est la décision ordinaire.
- Sans une telle règle, la demande DOIT être refusée :
  `decision=denied, reason=governed_no_policy`.
- La règle d'un agent qui n'est pas auteur ne compte pas.
- La réponse à `policy_query` porte `governed=<motifs>`.

Effet : une politique absente, ou refusée par le serveur (contradiction,
signature), ne laisse plus passer une action à risque. Les actions non
gouvernées gardent la politique par défaut.

### 6.4 Publier une politique : ordre de l'auteur et reçu signé (ajouté le 2026-11-25)

Un message de règles PEUT porter `INTENT_PAYLOAD.policy_seq=<entier>`.
Le champ est facultatif : sans lui, rien ne change.

**Ordre.**
- Le serveur DOIT refuser un message dont `policy_seq` n'est pas
  strictement supérieur au dernier `policy_seq` accepté de cet expéditeur.
  La réponse est `purpose=stale_policy, last_seq=<n>`, et rien n'est
  stocké. Deux messages authentiques livrés dans le désordre ne font donc
  pas gagner l'ancien.
- `policy_seq` exige une signature valide (sinon
  `policy_seq_requires_signature`). Il doit être un entier (sinon
  `policy_seq_invalid`).

**Reçu.** La réponse d'un message accepté qui porte `policy_seq` contient :

```
RECEIPT [sender=<expéditeur>, policy_seq=<n>, request_hash=sha256:<hex>, audit_hash=<hash du maillon>, signature=<128 hex>]
```

- `request_hash` est le SHA-256 des octets canoniques (section 4) du
  message **tel que le serveur l'a reçu**.
- `signature` est la signature Ed25519, par la clé du serveur, de :

```
cstl-receipt-v1
sender=<sender>
policy_seq=<policy_seq>
request_hash=<request_hash>
audit_hash=<audit_hash>
```

**Vérification par l'auteur.** L'auteur DOIT vérifier quatre choses :
- la signature, sous la clé du serveur obtenue hors bande ;
- `sender` ;
- `policy_seq` ;
- `request_hash`, comparé à l'empreinte de **son** message.

Sans reçu valide, la politique n'est peut-être pas en vigueur : elle a pu
être modifiée, perdue, refusée ou jugée trop ancienne. L'auteur la
republie alors avec une nouvelle séquence. Implémentation :
`CstlAgent.publish_policy` (Python), qui lève `PolicyNotConfirmed` après
plusieurs essais.

## 7. Autoriser avant d'agir

### 7.1 Demande

```
INTENT_PAYLOAD [purpose=action_request, sender=<agent>, receiver=server, action=<action>, request_id=<unique>, amount=<facultatif>, signature=...]
```

`action` DOIT être fait de 1 à 128 caractères parmi `A-Z a-z 0-9 _ . -`.
Sinon la demande est refusée (`decision=denied`,
`reason=missing_or_invalid_action`).

`request_id` DOIT être unique. Deux demandes identiques auraient le même
contenu, et la seconde serait prise pour un renvoi.

### 7.2 Réponse

`INTENT_PAYLOAD [purpose=action_authorization, ...]`, avec :
- `receiver` : l'agent ;
- `action` ;
- `decision` : `allowed` ou `denied` ;
- `reason` : par exemple `forbidden`, `no_prohibition` ou
  `default_deny_no_explicit_permission` ;
- `rule`, si une règle a décidé.

La réponse est suivie d'un bloc `AUDIT` (section 9).

Si `decision=allowed`, elle porte aussi le **jeton** :
- `request_id`, `issued_at`, `expires_at` (secondes unix) ;
- `audit_hash` : le `hash` du bloc `AUDIT` de cette décision ;
- `amount` : seulement si la demande en portait un ;
- `authorization` : la signature du serveur, 128 caractères hexadécimaux.

Le champ `server_key` renvoyé dans la réponse est indicatif. NE DOIT PAS
servir à vérifier le jeton : un faux serveur y mettrait sa propre clé.

### 7.3 Vérifier un jeton (système qui exécute l'action)

**Message signé par le serveur**, lignes séparées par `\n`, sans `\n`
final ; la ligne `amount` n'existe que si le jeton porte un montant :

```
cstl-authz-v1
agent=<receiver>
action=<action>
request_id=<request_id>
issued_at=<issued_at>
expires_at=<expires_at>
audit_hash=<audit_hash>
amount=<amount>
```

Le système qui exécute DOIT refuser, dans cet ordre :

| Verdict | Cas |
|---|---|
| `bad_signature` | signature invalide sous la clé publique du serveur, **obtenue hors bande** (fichier `<base>.server_key.pub`) |
| `wrong_action` | une autre action que celle du jeton |
| `wrong_agent` | un autre agent, si l'agent est connu |
| `wrong_amount` | le jeton ou l'exécution porte un montant, et les deux ne sont pas présents et numériquement égaux (`80` = `80.0`) |
| `not_yet_valid` / `expired` | hors de [`issued_at`, `expires_at`], à une tolérance d'horloge près |
| `already_used` | un `request_id` déjà exécuté |

Vecteurs : `conformance/vectors.json`, section `token`. Implémentations :
`cstl_guard.CstlGuard` (Python) et `server_key::verify_authorization`
(Rust). Côté Rust, l'usage unique est à la charge de l'appelant.

### 7.4 Arrêt d'urgence (ajouté le 2026-11-25)

```
INTENT_PAYLOAD [purpose=emergency_stop, sender=<auteur>, receiver=server, scope=all|agent:<nom>|action:<motif>, state=on|off, reason=..., policy_seq=<n>, signature=...]
```

- Seul un auteur de politique prouvé peut poser ou lever un arrêt. Sans
  `CSTL_POLICY_AUTHORS`, il est refusé (`reason=policy_authors_required`).
- `action:<motif>` : un nom exact ou `prefixe*`. `scope` vaut `all` par
  défaut, `state` vaut `on`.
- Tant qu'un arrêt couvre une demande, `action_request` répond
  `decision=denied, reason=emergency_stop, rule=<scope>`, **quelles que
  soient les règles**, et aucun montant n'est compté.
- `policy_seq` est obligatoire : un vieux message « off » rejoué ou
  retardé est refusé (`stale_policy`).
- **Arrêt de l'opérateur :** si le fichier `<base>.emergency_stop` existe
  (à côté de `<base>.server_key`, ou `CSTL_EMERGENCY_STOP_FILE`), tout est
  arrêté, avec `rule=operator`. Aucun message ne le lève, même signé par
  un auteur : seul l'opérateur de la machine retire le fichier. C'est le
  recours si une clé d'auteur est volée.
- La politique effective (section 8) porte `emergency_stop=<portées>` quand
  un arrêt concerne l'agent.
- **Réponse :** `purpose=emergency_stop_set, scope=..., state=...`, suivie
  d'un bloc `AUDIT` ; un refus : `purpose=control_denied` (section 5.5).
- **Limite :** comme pour la révocation, un jeton déjà délivré reste
  valable jusqu'à son expiration.

### 7.5 Accord humain (ajouté le 2026-11-25)

- Une action qui correspond à `CSTL_APPROVAL_ACTIONS` (noms exacts ou
  `prefixe*`) et que les règles permettent n'obtient pas de jeton
  directement. La réponse est `decision=denied, reason=approval_required,
  rule=approval, approval_id=<32 hex>`.
- Un approbateur (`CSTL_APPROVERS`, sinon les auteurs de
  `CSTL_POLICY_AUTHORS`) décide, signé, avec `policy_seq` :
  `INTENT_PAYLOAD [purpose=approval_decision, approval_id=<id>, decision=approve|reject, reason=..., policy_seq=<n>, ...]`.
  Réponse : `purpose=approval_recorded` et un bloc `AUDIT`. Refus
  (`control_denied`) : `not_authorized`, `self_approval` (l'agent qui a
  demandé), `approval_not_pending`, `approval_expired`, `approval_unknown`.
- Liste des demandes en attente, pour un approbateur :
  `purpose=approval_list` → `purpose=approvals`, un bloc
  `APPROVAL [approval_id, agent, action, amount, requested_at, expires_at]`
  par demande.
- L'agent redemande avec `approval_id=<id>`. Les règles et l'arrêt
  d'urgence sont réévalués. L'accord doit porter sur le même agent, la même
  action et le même montant (`approval_mismatch`), être donné
  (`approval_pending`, `approval_rejected`), dans le délai
  `CSTL_APPROVAL_TTL_SECS` compté depuis la demande (3600 par défaut,
  `approval_expired`), et n'avoir jamais servi (`approval_used`). Le jeton
  porte alors `approval_id` et `approved_by`.
- Une interdiction reste une interdiction : aucun accord n'est créé pour une
  action que les règles refusent. Sans approbateur configuré, l'action
  n'est jamais permise.

### 7.6 État d'un jeton en direct (ajouté le 2026-11-25)

Le système qui exécute peut demander si un jeton vaut **encore**, sans être
un agent enregistré (message non signé accepté) :

```
INTENT_PAYLOAD [purpose=token_status, sender=<guard>, receiver=server, audit_hash=<jeton.audit_hash>, nonce=<8 à 128 [A-Za-z0-9_.-]>]
```

Réponse : `INTENT_PAYLOAD [purpose=token_status, audit_hash, status, nonce,
checked_at, signature]`. `signature` est la signature Ed25519, par la clé du
serveur, de :

```
cstl-token-status-v1
audit_hash=<audit_hash>
status=<status>
nonce=<nonce>
checked_at=<secondes unix>
```

`status` : `valid`, `expired`, `revoked` (la clé de l'agent a été
révoquée), `stopped` (un arrêt d'urgence couvre l'agent ou l'action) ou
`unknown`. Le système DOIT refuser tout ce qui n'est pas `valid` avec une
signature correcte et son propre nonce, y compris une absence de réponse.

### 7.7 Comportement anormal (ajouté le 2026-11-25)

Avec `CSTL_ANOMALY_MAX_PER_MIN=N`, un agent qui envoie plus de `N`
`action_request` en 60 s déclenche un arrêt d'urgence `agent:<nom>`, posé
par `anomaly_detector` (section 7.4). Il reste en vigueur jusqu'à ce qu'un
auteur de politique le lève ; le compteur de l'agent repart alors de zéro.

## 8. Politique effective

```
INTENT_PAYLOAD [purpose=policy_query, sender=<agent>, receiver=server, request_id=<unique>, signature=...]
```

**Réponse :** `INTENT_PAYLOAD [purpose=policy, ...]`, avec :
- les nombres `required`, `permitted`, `forbidden` et `ignored` ;
- `authors_enforced` ;
- `default` ;
- `governed`, `emergency_stop` et `approval`, quand il y en a.

Elle est suivie d'un bloc `POLICY_RULE [action, decision, modality,
author, reason, ...]` par règle en vigueur. Une règle ignorée (auteur sans
autorité) apparaît dans `POLICY_IGNORED`.

## 9. Audit

- Chaque message traité et chaque décision d'autorisation DOIT produire un
  bloc `AUDIT [hash=sha256:<64 hex>, parent_hash=<hash précédent ou root>,
  seq=<n>]`.
- `seq` croît de 1 à chaque maillon ; `parent_hash` est le `hash` du
  maillon précédent.
- Un message refusé avant traitement (`status=error`) N'EST PAS audité.

Le serveur signe chaque maillon avec sa clé. Il DOIT refuser de démarrer
dans ces cas :
- un maillon a été modifié ;
- un maillon a été supprimé ;
- une signature est absente ;
- la chaîne est plus courte que son marqueur signé ;
- elle est plus ancienne que le témoin externe, quand il est configuré.

Le détail est dans `AGENTS.md`, sections « The same server key also signs
every link » et « Rollback ».

## 10. Erreurs

Toutes ces réponses portent `META [status=error]`, et aucune n'est auditée :

| `purpose` | Cause |
|---|---|
| `validation_error` | champ obligatoire absent ; `errors=` liste les codes `E3xx` |
| `parse_error` | message illisible |
| `security_rejected` | caractères ou structure dangereux |
| `signature_rejected` | section 5.2 |
| `schema_rejected` | relation mal typée, quand un `SCHEMA` existe |
| `tree_rejected` | arbre incomplet ou engagement de source non respecté |
| `connection_timeout` | rien de complet reçu à temps |
| `stale_policy` | `policy_seq` pas plus récent que le dernier accepté (section 6.4) |
| `policy_seq_invalid` / `policy_seq_requires_signature` | section 6.4 |
| `control_denied` | révocation, inscription de clé, arrêt d'urgence ou décision d'accord refusé (sections 5.5, 7.4 et 7.5) |
| `token_status_rejected` | `audit_hash` ou `nonce` illisible (section 7.6) |

## 11. Ce qui est figé en v1

Ne changeront pas sans une v2 :
- la syntaxe des sections 2 et 3 ;
- les octets canoniques (section 4) ;
- les champs et l'ordre du message de jeton (`cstl-authz-v1`) ;
- les valeurs de `purpose`, `decision` et des verdicts de jeton ;
- le message du reçu (`cstl-receipt-v1`).

Un serveur PEUT ajouter des blocs et des champs. Un client DOIT ignorer
ceux qu'il ne connaît pas.

## 12. Tester une implémentation

```bash
python3 conformance/run_server_conformance.py --port 5050 --server-key cstl_adn.db.server_key.pub
```

- Lancer le serveur sans `CSTL_DEV_MODE` et sans `CSTL_POLICY_AUTHORS`.
- Le testeur vérifie d'abord sa propre implémentation sur les vecteurs,
  puis exécute 16 contrôles :
  - C1 à C5 : identité et règles ;
  - C6 à C9 : décisions, jetons et plafonds ;
  - C10 et C11 : audit ;
  - C12 : politique ;
  - C13 : message incomplet ;
  - C14 : nom d'action invalide ;
  - C15 et C16 : ordre de l'auteur et reçu signé.
