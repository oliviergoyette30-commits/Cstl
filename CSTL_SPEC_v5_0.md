# CSTL v5.0.0 — Spécification complète unifiée

**Status**: v5.0.0, 22 juin 2026
**Author**: Olivier Goyette
**License**: Apache 2.0
**Supersedes**: CSTL_SPEC_v4.9.3, CSTL_SPEC_v4.0

Ce document est la référence canonique unique pour CSTL v5.0.0.
Il intègre la totalité de v4.9.3 et toutes les additions v5.0.

---

## 1. Scope et positionnement

CSTL est un format textuel de communication LLM-à-LLM. Chaque payload est
indépendamment parseable, porte son contenu sémantique complet, et ne requiert
aucune infrastructure partagée entre agents.

CSTL n'est pas un protocole de transport (cf. MCP, A2A) ni un langage de
requête (cf. G²CP). C'est un format de payload. L'objectif de conception est
la préservation des modalités déontiques, marqueurs d'incertitude, temporalité,
et force à travers les hops agent-à-agent — information que le texte libre perd
et que JSON Schema n'encode que par convention.

**Combinaison unique** (aucun format existant ne combine les quatre) :
- Modalités déontiques natives : MUST, MUST_NOT, MAY, IFF
- Incertitude quantifiée : aléatoire vs épistémique, σ par relation
- Traçabilité de provenance : produced_by, PARENT_HASH, SHA-256 canonique
- Parser déterministe : O(n), zéro LLM requis pour validation

---

## 2. Structure d'un document

```
#!CSTL v5.0.0 MODE=A
META [ ... ]
(RULE) assistant MUST respond_exclusively_in_cstl
(RULE) assistant MUST_NOT output_prose_or_explanation
INTENT_PAYLOAD [ ... ]
CONSTRAINTS [ ... ]
UNCERTAINTY [ ... ]
DEFINE ... AS ... [ ... ]
RELATIONS [ ... ]
DECISION: valeur [sigma=X]
---END---
```

Le hashbang utilise des espaces, pas des underscores : `#!CSTL v5.0.0 MODE=A`.
`MODE=A` désigne le mode standalone. Les modes B et C (CASTLE, dictionnaire
partagé) sont réservés et non implémentés dans cette version.

**Grammaire BNF — ordre canonique de l'encodeur :**

```ebnf
document      ::= header meta rule_block* intent_payload?
                  constraints? uncertainty? define_block*
                  relations_block? decision? trailer ;

header        ::= "#!CSTL" SP version SP "MODE=A" NEWLINE ;
version       ::= "v" digit+ "." digit+ ("." digit+)? ;
trailer       ::= "---END---" ;
```

**Distinction encodeur / parser :**

- L'encodeur DOIT émettre les blocs dans l'ordre canonique ci-dessus.
- Le parser DOIT accepter les blocs dans n'importe quel ordre après META
  (loi de Postel pour la rétrocompatibilité inter-versions).
- META est obligatoire et doit apparaître immédiatement après le header
  aussi bien à l'encodage qu'au parsing. META en position non-initiale → W502.

---

## 3. Sémantique formelle — Logique déontique (addition v5.0)

### 3.1 Fondement théorique

Les opérateurs déontiques CSTL sont fondés dans la Logique Déontique Standard
(SDL) formalisée par von Wright (1951) et étendue par McNamara (2006).

SDL est un système de logique modale avec trois opérateurs primitifs :

| Opérateur SDL | Opérateur CSTL | Sémantique |
|---|---|---|
| O(φ) — Obligatoire | MUST | φ est obligatoire dans tous les mondes idéaux accessibles |
| P(φ) — Permis | MAY | φ est vrai dans au moins un monde idéal accessible |
| F(φ) — Interdit | MUST_NOT | ¬φ est obligatoire ; équivalent à O(¬φ) |

**Sémantique de Kripke** : Un payload CSTL est interprété sur un cadre de
Kripke ⟨W, R, V⟩ où W est un ensemble de mondes possibles, R est une relation
d'accessibilité (relation monde-idéal), et V est une fonction de valorisation.

- `MUST φ` est vrai en w ssi φ est vrai dans tous les w' accessibles depuis w
- `MAY φ` est vrai en w ssi φ est vrai dans au moins un w' accessible depuis w
- `MUST_NOT φ` est équivalent à `MUST ¬φ`

**Axiomes satisfaits par les opérateurs déontiques CSTL (suivant SDL) :**

```
(D)  O(φ) → P(φ)                      -- obligation implique permission
(C)  O(φ) ∧ O(ψ) → O(φ ∧ ψ)          -- agglomération
(N)  O(⊤)                              -- nécessité des tautologies
(K)  O(φ → ψ) → (O(φ) → O(ψ))        -- distributivité
```

L'axiome D est l'axiome caractéristique de SDL (von Wright, 1951).
L'axiome T (O(φ) → φ, factitivité) n'est PAS satisfait dans SDL : une
obligation peut ne pas être réalisée dans le monde réel. CSTL suit SDL
standard sur ce point pour MUST/MAY/MUST_NOT.

**Limitation honnête** : CSTL n'implémente pas un prouveur de théorèmes.
Le validator détecte les violations syntaxiques ; la cohérence modale
sémantique est la responsabilité de l'encodeur.

**Mapping σ → force déontique** :
`sigma=1.0` correspond à O(φ) strict (obligation absolue) ;
`sigma=0.5` à une recommandation faible. Extension défaisable : Prakken & Sartor (1997).

### 3.2 Opérateurs épistémiques (v5.0)

Les opérateurs KNOWS/BELIEVES/ASSUMES/DOUBTS opèrent dans un cadre
épistémique distinct du cadre déontique. Dans ce cadre épistémique,
KNOWS satisfait l'axiome T (K_a(φ) → φ) : si un agent sait φ, alors φ
est vrai. BELIEVES ne satisfait pas T, permettant la faillibilité.

Cette distinction (fondée dans Hintikka, 1962 ; Fagin et al., 1995) est
intentionnelle : KNOWS encode la connaissance factuelle, BELIEVES la croyance
justifiée. Ce sont deux cadres modaux séparés coexistant dans un même payload.

| Opérateur | Axiome T | Sens épistémique | σ recommandé |
|---|---|---|---|
| KNOWS | Oui (K_a(φ) → φ) | Certitude factuelle | ≥ 0.80 |
| BELIEVES | Non | Croyance justifiée mais faillible | 0.40–0.85 |
| ASSUMES | Non | Hypothèse de travail | 0.30–0.75 |
| DOUBTS | Non | Faible confiance | ≤ 0.50 |

Le validator émet W604 si KNOWS avec σ < 0.8, W605 si DOUBTS avec σ > 0.5.

---

## 4. Bloc META

```ebnf
meta       ::= "META" "[" meta_field ("," meta_field)* "]" ;
meta_field ::= key "=" value ;
```

Note : les noms de champs META ne portent pas d'annotations de type inline
(pas de `:float`, `:enum`, `:bool`). Le type est défini par cette spec.

### 4.1 Champs obligatoires

| Champ | Type | Description |
|---|---|---|
| `encoder` | string | Rôle de l'agent, ex: `Agent_CLAUDE` |
| `produced_by` | model_id | Modèle réel, ex: `anthropic/claude-opus-4-8` ou `UNKNOWN` |
| `sigma` | float 0.0–1.0 | Confiance déclarée du payload |
| `PARENT_HASH` | hash | `sha256:<64hex>` ou `root` |

### 4.2 Champs optionnels

| Champ | Type | Description |
|---|---|---|
| `TURN` | int | Position dans un échange multi-tour |
| `TIMESTAMP` | iso8601 | Horodatage d'encodage |
| `CONVERSATION_ID` | string | Identifiant de session |
| `DOMAIN` | string | Domaine sémantique : medical, legal, corporate, diplomatic… |
| `ACTION` | enum | Action demandée (whitelist) |

Note : `RESPONSE_FORMAT` et `NO_PROSE` sont remplacés par les blocs RULE
depuis v4.9.3 (voir §5). Ils restent acceptés pour rétrocompatibilité mais
ne doivent pas être utilisés seuls — les blocs RULE sont la référence.

### 4.3 Sémantique de `produced_by`

Forme canonique : `org/model-version` (ex: `openai/gpt-5.5`).
`produced_by=UNKNOWN` est légal pour les modèles open-weight sans RLHF
de marque fiable.
Proxy : `proxy/org -> org/model-version`.

Six règles de validation (PB1–PB6, distinctes des règles générales R1–R13) :

| Code | Nom | Condition |
|---|---|---|
| PB1 | IDENTITY_MISMATCH | encoder et produced_by incohérents |
| PB2 | REDUNDANT | produced_by duplique encoder sans info supplémentaire |
| PB3 | CLEAN | Forme canonique, aucun warning |
| PB4 | PATCH_T4 | Normalisation appliquée automatiquement |
| PB5 | PROXY | Forme `proxy/org -> org/model` détectée |
| PB6 | PROXY_MASKED_BACKEND | Proxy sans backend déclaré |

---

## 5. Blocs RULE — directives de réponse

```ebnf
rule_block ::= "(" "RULE" ")" SP subject SP rule_modality SP directive NEWLINE ;
rule_modality ::= "MUST" | "MUST_NOT" | "MAY" | "SHOULD" ;
subject       ::= "assistant" | identifier ;
directive     ::= identifier (SP identifier)* ;
```

Exemples normatifs :

```
(RULE) assistant MUST respond_exclusively_in_cstl
(RULE) assistant MUST_NOT output_prose_or_explanation
(RULE) assistant MUST begin_response_with_hashbang_line
(RULE) assistant MUST end_response_with_triple_dash_end_marker
(RULE) assistant MUST_NOT decode_or_translate_payload
(RULE) assistant MUST treat_input_as_chain_continuation
(RULE) assistant MUST copy_all_rules_to_output
```

**Normative v4.9.3** : les blocs RULE sont le mécanisme principal de contrôle
des réponses. Les champs META `RESPONSE_FORMAT` et `NO_PROSE` sont insuffisants
— les modèles receveurs les traitent comme métadonnées passives. Les directives
`(RULE) assistant MUST ...` sont empiriquement fiables cross-vendor.

Un payload qui requiert une réponse CSTL-only DOIT utiliser des blocs RULE.

---

## 6. Bloc INTENT_PAYLOAD

```ebnf
intent_payload ::= "INTENT_PAYLOAD" "[" intent_attr* "]" ;
intent_attr    ::= intent_key "=" value NEWLINE? ;
intent_key     ::= "reason" | "sender" | "receiver" | "purpose"
                 | "priority" | "context" ;
priority_val   ::= "critical" | "high" | "normal" | "low" ;
```

Les clés listées sont les clés officielles. Toute autre clé est acceptée
avec warning R5. La liste est exhaustive pour les clés sans warning.

---

## 7. Bloc CONSTRAINTS — modalités déontiques

```ebnf
constraints     ::= "CONSTRAINTS" "[" constraint_line* "]" ;
constraint_line ::= paren_form | bracket_form ;

paren_form   ::= "(" modality ")" SP subject SP operator SP object
                 (SP "[" attr_list "]")? NEWLINE ;

bracket_form ::= "[" modality "]" SP subject SP operator SP object
                 (SP "[" attr_list "]")? NEWLINE ;

modality ::= "MUST" | "MUST_NOT" | "NOT" | "MAY" | "SHOULD"
           | "IF" | "IFF" | "UNLESS" | "REQUIRE" | "FORBID" ;
```

Les deux formes `(MUST)` et `[MUST]` sont acceptées. La forme `(MUST)` est
la forme canonique v5.0 ; `[MUST]` est conservée pour rétrocompatibilité v4.x.

---

## 8. Bloc UNCERTAINTY — statuts épistémiques

```ebnf
uncertainty_block ::= "UNCERTAINTY" "[" uncertainty_item* "]" ;
uncertainty_item  ::= identifier SP status
                      (SP "[" "sigma" "=" float "]")? NEWLINE ;
status ::= "ESTIMATED" | "INFERRED" | "UNKNOWN" | "MEASURED" ;
```

| Statut | Sens | sigma requis |
|---|---|---|
| `UNKNOWN` | Information absente, irrécupérable | Non |
| `ESTIMATED` | Valeur approximative avec confiance déclarée | Oui |
| `INFERRED` | Déduit par l'encodeur, pas explicite dans la source | Oui |
| `MEASURED` | Valeur mesurée empiriquement | Oui |

---

## 9. Bloc DEFINE — entités typées

```ebnf
define_block ::= "DEFINE" SP identifier SP "AS" SP entity_type
                 (SP "[" attr_list "]")? NEWLINE ;
entity_type  ::= "human" | "agent" | "document" | "system" | "concept"
               | "place" | "event" | "infrastructure" | "threat"
               | "deliverable" ;
```

### Les 10 types officiels

| Type | Exemple |
|---|---|
| `human` | superviseur, expert, patient |
| `agent` | modèle IA, sous-agent |
| `document` | contrat, rapport, protocole |
| `system` | pipeline, infrastructure logique, EHR |
| `concept` | équité, conformité, traitement |
| `place` | lieu géographique, datacenter |
| `event` | sommet, incident, essai clinique |
| `infrastructure` | cluster serveur, réseau |
| `threat` | fuite de données, risque, contraindication |
| `deliverable` | livrable final, rapport d'audit |

Type hors de cette liste → **warning R5**. Le type inconnu est accepté avec warning.

---

## 10. Bloc RELATIONS — graphe sémantique

**Total : 36 opérateurs officiels** (21 core v4 + 15 v5.0).

```ebnf
relations_block ::= "RELATIONS" "[" relation_line* "]" ;
relation_line   ::= "(" subject ")" SP operator SP object
                    (SP "[" attr_list "]")? NEWLINE ;
```

### 10.1 Opérateurs core v4 (21 opérateurs)

| Famille | Opérateurs (21 total) |
|---|---|
| Causalité (5) | `ARR`, `ARR.CREATE`, `ARR.JOIN`, `ARR.PRODUCE`, `ARR.ACCESS` |
| Intentionnalité (4) | `INTENT`, `MAINTAIN`, `TRANSFORM`, `RESIST` |
| Dynamiques (4) | `AMP`, `INH`, `PRESSURE`, `CATALYZE` |
| Relationnel (3) | `MUTUAL`*, `TRANSMIT_FAITHFUL`, `TRANSMIT_INFER` |
| Actes de langage (5) | `COMMAND`, `ASK`, `STATE`, `PERFORM`, `RECOMMEND` |

*`MUTUAL` est déprécié en v5.0 — voir §10.2.

### 10.2 Opérateurs relationnels v5.0 — MUTUAL déprécié (6 opérateurs)

`MUTUAL` encodait 6 sémantiques distinctes sous un seul opérateur, dont deux
non-symétriques (POSSESSES, COMPARES). Remplacé par 6 opérateurs non-ambigus :

| Opérateur v5.0 | Symétrie | Remplace MUTUAL quand |
|---|---|---|
| `EQUALS` | Symétrique | Identité ou équivalence |
| `POSSESSES` | Asymétrique | Possession ou containment |
| `RESEMBLES` | Symétrique | Similarité ou analogie |
| `CO_LOCATES` | Symétrique | Co-localisation |
| `OPPOSES` | Symétrique | Antagonisme ou opposition |
| `COMPARES` | Asymétrique | Comparaison explicite |

W601 émis à chaque occurrence de MUTUAL avec guide de migration.
MUTUAL reste syntaxiquement accepté pour rétrocompatibilité.

### 10.3 Opérateurs logiques v5.0 (2 opérateurs)

| Opérateur | Sens | Symétrie | Propriété |
|---|---|---|---|
| `ENTAILS` | A ⊨ B : A implique logiquement B | Asymétrique | Transitif — W603 si fermeture incomplète |
| `CONTRADICTS` | A ⊥ B : incohérence mutuelle | Anti-symétrique | W602 si A⊥B et B⊥A tous deux déclarés |

### 10.4 Opérateurs épistémiques v5.0 (4 opérateurs)

| Opérateur | Sens | Axiome T | σ recommandé |
|---|---|---|---|
| `KNOWS` | Certitude factuelle K_a(φ) | Oui | ≥ 0.80 |
| `BELIEVES` | Croyance justifiée B_a(φ) | Non | 0.40–0.85 |
| `ASSUMES` | Hypothèse de travail | Non | 0.30–0.75 |
| `DOUBTS` | Faible confiance | Non | ≤ 0.50 |

W604 : KNOWS avec σ < 0.8. W605 : DOUBTS avec σ > 0.5.

### 10.5 Opérateurs temporels v5.0 (3 opérateurs) — subset Allen (1983)

| Opérateur | Relation Allen | Sens formel |
|---|---|---|
| `BEFORE` | before (b) | end(A) < start(B) |
| `AFTER` | after (bi) | start(A) > end(B)  (inverse de BEFORE) |
| `DURING` | during (d) | start(B) ≤ start(A) ∧ end(A) ≤ end(B) |

E701 (erreur dure) : A BEFORE B et A AFTER B déclarés pour la même paire.
Extensions futures v5.x : MEETS, OVERLAPS, STARTS, FINISHES (Allen 1983 complet).

---

## 11. Blocs AGREEMENT / DISAGREEMENT

```ebnf
agreement_block    ::= "AGREEMENT_BLOCK" "[" dissent_item* "]" ;
disagreement_block ::= "DISAGREEMENT_BLOCK" "[" dissent_item* "]" ;

dissent_item ::= dissent_type SP label (SP "[" attr_list "]")? NEWLINE ;

label ::= identifier ("_" identifier)* ;

dissent_type ::= "STRENGTH" | "GAP" | "DISPUTE" | "PARTIAL_DISPUTE"
               | "CONCERN" | "CAUTION" | "ALTERNATIVE" | "REJECT"
               | "VETO" | "SELF_CRITIQUE" | "AGREEMENT" | "RECOMMEND" ;
```

`label` est un identifiant composé décrivant l'objet du dissensus (ex:
`multi_hop_curve`, `single_auditor`, `renal_risk_assessment`).

`DISAGREEMENT_BLOCK` est requis pour tout payload avec `ACTION=critique`.
Au moins une primitive de dissensus est obligatoire dans ce cas.

Ces blocs encodent le dissensus structuré dans les échanges multi-agents,
permettant l'audit des positions divergentes à travers les hops.

---

## 12. Bloc DECISION

```ebnf
decision ::= "DECISION" (":" | "=") value (SP "[" attr_list "]")? NEWLINE ;
```

Exemples :
```
DECISION: ratify [sigma=0.92]
DECISION=reject [sigma=0.88, rationale=insufficient_evidence]
```

Les deux formes (`:` et `=`) sont acceptées. La forme `:` est canonique.

---

## 13. Attributs `[ ]`

Maximum **9 attributs par relation** (théorème k=9 : zéro collision sur ≤10⁶
relations ; k=14 pour ≥10¹² relations — croissance logarithmique).

```ebnf
attr_list   ::= attribute ("," SP? attribute){0,8} ;
attribute   ::= sigma_attr | tau_attr | layer_attr | weight_attr
              | id_attr | coref_attr | identifier "=" value ;

sigma_attr  ::= ("sigma" | "σ") "=" float ;
tau_attr    ::= ("tau"   | "τ") "=" time_val ;
layer_attr  ::= ("layer" | "δ") "=" layer_val ;
weight_attr ::= ("weight"| "ω") "=" weight_val ;
id_attr     ::= ("id"    | "ι") "=" identifier ;
coref_attr  ::= "coref_with" "=" identifier ;

time_val    ::= "past" | "present" | "future" | "p" | "n" | "f" ;
layer_val   ::= "bedrock" | "deep" | "shallow" | "surface"
              | "b" | "d" | "s" | "su" ;
weight_val  ::= "positive" | "negative" | "neutral" | "+" | "-" | "°" ;
float       ::= digit+ ("." digit+)? ;   -- clampé à [0.0, 1.0]
```

Les symboles compacts σ, δ, τ, ω, ι sont FIXES et non redéfinissables (R6).
Les deux formes (compacte et verbeuse) DOIVENT être supportées par le parser.

`coref_with=eXXX` référence une entité définie dans un DEFINE antérieur.
Utilisé pour la validation cross-bloc (R8).

---

## 14. Mécanismes de relay — GRAMMAR_PRIMER et COPIED_RULES

Pour atteindre 96-98% de fidélité sur une chaîne multi-LLM, deux mécanismes
sont requis. Sans eux, l'intégrité brute est 35-60%.

### 14.1 GRAMMAR_PRIMER

Bloc de rappel syntaxique inséré après les RULE blocks, destiné aux modèles
sans prior CSTL fort. Exemple minimal :

```
GRAMMAR_PRIMER [
hashbang=#!CSTL v5.0.0 MODE=A,
meta=META [encoder=X, produced_by=Y, sigma=0.9, PARENT_HASH=Z],
rule=(RULE) assistant MUST respond_exclusively_in_cstl,
constraint=(MUST) subject OPERATOR object [sigma=0.9],
relation=(subject) OPERATOR object [sigma=0.9, τ=present, id=rXXX],
uncertainty=field ESTIMATED [sigma=0.72],
trailer=---END---
]
```

### 14.2 COPIED_RULES

Les 13 règles de validation (R1–R13) sont copiées directement dans le payload
sous forme de blocs `RULE_N`. Chaque agent reçoit les règles avec le contenu
et les transmet au hop suivant — gouvernance auto-propagée.

```
(RULE) assistant MUST copy_all_rules_to_output
(RULE) assistant MUST preserve_all_existing_relations
(RULE) assistant MUST update_encoder_and_parent_hash
(RULE) assistant MUST add_minimum_3_new_relations
(RULE) assistant MUST_NOT delete_existing_entities
(RULE) assistant MUST_NOT translate_or_summarize_payload
```

---

## 15. Forme canonique et hachage

### 15.1 Ou vit reellement cette section (corrige le 2026-09-03)

Cette section decrivait auparavant un algorithme (`src/canonical.rs`,
canonicalisation de TEXTE BRUT en 5 regles, puis SHA-256) qui n'a jamais
ete appele par le serveur en production -- decouvert le 2026-09-03 en
verifiant l'effet reel d'un fix NFC applique a ce fichier. `src/canonical.rs`
a ete supprime le meme jour: du code mort qui pretendait etre la
reference alors qu'une implementation SEPAREE et DIFFERENTE faisait
deja tout le travail reel.

Le hash canonique reellement utilise en production -- cle primaire de
`adn_store`, chainon de `HashChain` -- est `server/audit.rs::canonical_hash()`.
Difference structurelle importante: il n'opere PAS sur le texte brut du
payload, mais sur les champs DEJA PARSES (`CstlPayload`: version, mode,
meta, intent, relations). Consequence: les regles de canonicalisation de
texte brut (fins de ligne, espaces multiples, espaces finaux) n'ont pas
de sens pour lui -- le parser (`server/parser.rs`) les a deja neutralisees
en extrayant les champs. Les deux regles qui comptent reellement, et qui
sont reellement appliquees:

1. **Tri lexicographique des champs** -- via `BTreeMap`, sur META (moins
   `PARENT_HASH`, exclu pour eviter une dependance circulaire du hash sur
   lui-meme), INTENT, et chaque bloc RELATION.
2. **Normalisation Unicode NFC** -- appliquee a chaque cle et valeur avant
   hachage (corrige le 2026-09-03, meme trouvaille NFC que ci-dessus,
   appliquee cette fois au code qui tourne reellement).

### 15.2 Hash canonique

`server/audit.rs::canonical_hash()` calcule SHA-256 sur la concatenation
triee et NFC-normalisee des champs et produit un digest 256-bit
(64 caracteres hex), prefixe `sha256:`.

La largeur 256-bit a ete ratifiee en Session #7 : la borne anniversaire sur
128 bits est insuffisante (2^64 operations sous attaque deliberee).

### 15.3 Verification executable

Les proprietes ci-dessus (determinisme, invariance d'ordre des champs
META, equivalence NFC, exclusion de PARENT_HASH) sont verifiees par des
tests reels, executes par `cargo test --lib server::audit`, pas par des
exemples statiques dans ce document (les vecteurs #1/#2 precedemment
listes ici citaient des hash calcules par l'algorithme supprime de
`src/canonical.rs` -- ils ne correspondent a AUCUNE sortie du code
reellement en production, et ont ete retires pour ne pas induire en
erreur):

- `test_hash_is_deterministic`
- `test_hash_changes_with_content`
- `test_parent_hash_excluded_from_hash`
- `test_nfc_equivalent_field_values_produce_same_audit_hash`
- `test_chain_links_correctly`
- `test_integrity_detects_break`

toutes dans `src/server/audit.rs`.

---

## 16. Codes d'erreur

### 16.1 Codes v4.9.3 (21 codes normatifs, spec Python d'origine)

| Plage | Catégorie |
|---|---|
| E101–E105 | Structure parser |
| E106–E112 | Mutualité (7 formes) |
| E201–E205 | Sécurité |
| E301–E304 | Typage |
| E401–E404 | Quotas de ressources |
| W501–W503 | Warnings produced_by |

### 16.2 Mutualité E106–E112 (spec Python v4.9.3 — JAMAIS implémentée dans le moteur Rust)

`SessionValidator` vérifie ces 7 formes à travers un ensemble de payloads.
Le graphe de dépendance DOIT être un DAG (acyclique dirigé sans cycle).

| Code | Forme | Description |
|---|---|---|
| E106 | CircularHashRef | A→ref=sha256:B, B→ref=sha256:A |
| E107 | CircularAgree | A agrees B, B agrees A |
| E108 | CircularParentHash | A.parent=B, B.parent=A |
| E109 | MutualIdentitySwap | Gemini→OpenAI, GPT→Gemini |
| E110 | MutualArbitration | A asks B, B asks A |
| E111 | CircularDecision | A.decision dépend de B, B de A |
| E112 | MutualProducedBy | A.produced_by=B, B.produced_by=A |

**Correction honnête (audit du repo, 2026-09-04) : ce `SessionValidator` n'existe nulle
part dans le code Rust qui tourne réellement.** Cette section documente une conception
de la spec Python v4.9.3, jamais portée. Pire : le moteur Rust réellement câblé
(`src/semantic.rs`) **réutilise les codes E107, E108 et E109 pour des vérifications
totalement différentes** — voir §16.4 ci-dessous. C'est une vraie collision de
nomenclature, pas une simple omission : `E107`/`E108`/`E109` ne signifient PAS la même
chose selon que ce document ou le moteur Rust les émet. Non résolue à ce jour
(renuméroter l'un des deux casserait la compatibilité de tout consommateur qui
matcherait déjà sur ces chaînes) — un lecteur ou un intégrateur ne doit jamais assumer
la signification d'un de ces trois codes sans vérifier lequel des deux systèmes l'a
émis.

### 16.3 Codes v5.0 (spec Python v4.9.3 — statut d'implémentation Rust variable, voir §16.4)

| Code | Sévérité | Condition |
|---|---|---|
| E701 | error | Contradiction temporelle : A BEFORE B et A AFTER B pour même paire |
| W601 | warning | MUTUAL déprécié — guide de migration fourni |
| W602 | warning | CONTRADICTS redondant : A⊥B et B⊥A tous deux déclarés |
| W603 | warning | Fermeture transitive ENTAILS incomplète |
| W604 | warning | KNOWS avec σ < 0.8 — considérer BELIEVES ou ASSUMES |
| W605 | warning | DOUBTS avec σ > 0.5 — considérer BELIEVES |

Ces 6 codes-ci, contrairement à ceux de §16.2, correspondent effectivement à des
checks réels dans `src/semantic.rs` (E701/W602/W603/W604/W605 branchés sur le chemin
serveur TCP depuis le 2026-09-04 — voir §16.4 ; W601 branché depuis plus longtemps).

### 16.4 Codes réellement émis par le moteur Rust (`src/semantic.rs`, câblé live) — ajout honnête 2026-09-04

Section absente des révisions précédentes de ce document : les codes ci-dessous sont
ceux que `SemanticValidator::validate()` produit réellement sur chaque payload reçu
par le serveur TCP (`server/handler.rs`), avertissement seul sauf mention contraire
(un `SEMANTIC_WARNING` n'empêche jamais `status=processed`, sauf E107 qui est
bloquant — seul code de ce tableau qui rejette le payload).

Exception au tableau, ajoutée le 2026-09-05 : `R8` n'est PAS émis par
`SemanticValidator` — il vient directement de `server/validator.rs::
check_coref_with_references(payload: &CstlPayload)`, qui compare `relations` et
`defines` (deux champs de `CstlPayload`) entre eux. `SemanticValidator` ne connaît
que `Relation` (jamais `defines`), donc ce check ne pouvait structurellement pas y
vivre. Listé ici quand même parce que `server/handler.rs` l'agrège dans le même flux
`SEMANTIC_WARNING` que les codes ci-dessous, avec la même politique (avertissement
seul). Voir §19 pour l'historique complet de sa reconstruction.

| Code | Sévérité | Condition réelle (moteur Rust) |
|---|---|---|
| E101 | error (non bloquant, warning affiché) | Opérateur `RELATION.type` hors whitelist officielle/domaine/déprécié |
| W601 | warning | `MUTUAL` déprécié |
| E107 | **error, bloquant** | Axiome D SDL : `MUST`/`REQUIRE` et `MUST_NOT`/`FORBID` sur le même (sujet, objet) dans le même payload — **PAS** la même signification qu'E107 en §16.2 |
| E110 | **error, bloquant** | Axiome K SDL (distributivité, `semantic.rs::check_axiom_k_entailment`, ajouté 2026-09-05) : `MUST`/`REQUIRE` sur (S, A), une chaîne `ENTAILS` (transitive, intra-payload) reliant A à B, et `MUST_NOT`/`FORBID` sur (S, B) — même sujet S, objets A≠B différents reliés par une chaîne `ENTAILS` que le payload affirme lui-même. Détecte une classe de contradiction qu'E107 seul ne voit pas (E107 exige le même objet exact) ; exclut explicitement A=B (déjà couvert par E107, pas de doublon) et exige le même sujet (deux agents avec des obligations opposées sur des propositions liées ne sont pas une contradiction d'auteur) |
| E108 | warning | Contradiction temporelle : `MUST_NOT(S,O,τ)` et `PERFORM(S,O,τ)` au même instant — **PAS** la même signification qu'E108 en §16.2 |
| E109 | warning | `AMP` et `INH` déclarés sur la même paire (sujet, objet) — **PAS** la même signification qu'E109 en §16.2 |
| E701 | warning | Contradiction temporelle : `BEFORE`/`AFTER` incohérents pour la même paire |
| W502 | warning | `MAINTAIN` avec `tau=p`/`p_past` |
| W503 | warning | `tau` hors des valeurs valides (`p`, `n`, `f`, `p_past`, `n_present`, `f_future`) |
| W504 | warning | Dépassement du seuil d'attributs (voir R10, §19) |
| R8 | warning | `coref_with=<id>` sans DEFINE correspondant dans le même payload (`server/validator.rs::check_coref_with_references`, ajouté 2026-09-05) |
| R9 | warning | Valeur d'attribut hors ontologie (§13) |
| R10 | warning | >9 attributs sur une relation |
| W602 | warning | `CONTRADICTS` redondant (symétrie déjà impliquée) |
| W603 | warning | Fermeture transitive `ENTAILS` incomplète |
| W604 | warning | `KNOWS` avec σ < 0.8 |
| W605 | warning | `DOUBTS` avec σ > 0.5 |
| E702 | warning | **`src/execution_lab.rs`, pas `semantic.rs`** — cycle temporel : une CHAÎNE de relations `BEFORE`/`AFTER` sur PLUSIEURS paires de sujets qui revient sur son point de départ (ex. A BEFORE B, B BEFORE C, C BEFORE A). `BEFORE`/`AFTER` sont normalisés en une seule direction avant construction du graphe (A BEFORE B ⟺ B AFTER A), réutilise le même algorithme de détection de cycle par backtracking (`dfs_find_cycle`) que les cycles `part_of`/`located_in`. Vérifié contre le payload courant ET tout l'historique de l'ADN store, via `check_consistency_with_history` (`ConsistencyReport.temporal_cycles`), même politique que le cycle `part_of`/`located_in` (voir README.md, section "Future Architecture — Level 4"). **Ajouté le 2026-09-05**, distinct d'E701 ci-dessus (E701 = incohérence PAIRWISE, même paire, même payload ; E702 = cycle sur PLUSIEURS paires, potentiellement réparties entre payloads) |

Codes de validation structurelle/format (`src/server/validator.rs`, distinct de
`semantic.rs`) : `E301`–`E307`, `E309`, `E310` (E308 a existé puis a été retiré —
volontairement absent, un check cassé a été supprimé plutôt que réparé ; ne réapparaît
jamais). Ce sont des codes DIFFÉRENTS de la plage E301–E304 "Typage" du v4.9.3
documentée en §16.1, qui n'a jamais été portée telle quelle.

---

## 17. Profil de sécurité

| Code | Sévérité | Condition |
|---|---|---|
| SEC_Q1 | warning | Non-ASCII en position de keyword (attaque homoglyph potentielle) |
| SEC_Q2 | warning | Caractères zero-width (U+200B–U+200F etc.) détectés et supprimés |
| SEC_Q3 | warning | Caractères de contrôle bidirectionnels hex-escapés dans audit |
| SEC_Q4 | error | Injection de META imbriqué détectée |
| SEC_Q5 | error | Profondeur d'imbrication max 32 dépassée |

Note : SEC_Q3 couvre les caractères U+202A–U+202E et U+2066–U+2069.
`CSTLTruncationError` est levé explicitement sur entrée tronquée.

---

## 18. Arbitrage (Session #9)

Sept blocs CSTL spécifient la résolution de deadlock entre agents :

| Bloc | Rôle |
|---|---|
| `DEADLOCK_DECLARE` | Déclare un deadlock détecté |
| `ARBITRATION_REQUEST` | Demande d'arbitrage à un tiers |
| `ARBITRATION_RULING` | Décision de l'arbitre |
| `ARBITRATION_APPEAL` | Appel de la décision |
| `ARBITRATION_FINALIZE` | Finalisation irrévocable |
| `DEADLOCK_TRIGGER` | Déclencheur automatique de procédure |
| `ARBITRATION_TELEMETRY` | Métriques de l'arbitrage |

Seuil de deadlock normatif : **3 rounds**.

Un bloc `IDENTITY_ALERT` supporte la détection d'imposteur. Cas empirique :
un Llama-3-70B s'est présenté comme Claude en Session #9 — l'imposture a été
détectée via un `PARENT_HASH` invalide.

---

## 19. Règles de validation R1–R13

| Règle | Sévérité | Description | Statut impl. |
|---|---|---|---|
| R1 | error | IDs uniques (`id=eXXX`) dans le document | ✅ vérifié |
| R2 | warning | Version présente dans le header | ✅ vérifié |
| R3 | warning | sigma hors [0,1] → clampé avec warning | ✅ vérifié |
| R4 | info | Ordre canonique des blocs recommandé | ✅ |
| R5 | warning | Opérateur ou type d'entité hors officiels | ✅ vérifié |
| R6 | fixed | Symboles σδτωι non redéfinissables par l'utilisateur | ✅ vérifié |
| R7 | error | DEFINE avec crochets malformés → dropped + warning | ✅ câblé live (`server/parser.rs`, 2026-09-05) |
| R8 | warning | Références coref_with validées contre les DEFINE existants | ✅ reconstruit + câblé live (2026-09-05, portée intra-payload — voir note ci-dessous) |
| R9 | warning | Valeurs d'attributs non conformes à la whitelist §13 | ✅ câblé live (`semantic.rs::check_attribute_ontology`, 2026-09-04) |
| R10 | error | >9 attributs par relation → warning + drop du surplus | ✅ câblé live (`semantic.rs::check_attribute_bombing`, 2026-09-04) |
| R11 | error | Clé META dupliquée → invalidation du document | ✅ vérifié |
| R12 | error | Contenu textuel après `---END---` → truncation error | ✅ vérifié |
| R13 | warning | Version hashbang non reconnue (ni v4.9.x ni v5.0.x) | ✅ vérifié |

**Historique — R8 retiré le 2026-09-04** : ce check reposait sur
`defined_entities()`/`check_undefined_entity_reference()` dans `semantic.rs`, qui
opéraient sur le système `ast::Block` — retiré le même jour (100% mort, aucun
producteur réel n'a jamais construit de `Block` en dehors de ses propres tests, voir
CHANGELOG). R9/R10, en revanche, opéraient déjà sur `Relation` (jamais sur `Block`) et
ont été branchés sur le chemin TCP réel le même jour, aux côtés de 9 autres checks
jusque-là seulement testés en isolation (voir §16.4).

**Reconstruction R8 (2026-09-05)** : avant de pouvoir reconstruire quoi que ce soit,
il a fallu constater que les blocs DEFINE n'existaient MÊME PAS côté serveur — le
parser réel (`server::parser::parse_payload`) ne reconnaissait que META,
INTENT_PAYLOAD et RELATION ; DEFINE n'apparaissait nulle part dans `src/`. R8 était
donc structurellement impossible à reconstruire dans QUELQUE forme que ce soit, pas
seulement privé de son ancienne implémentation. Corrigé en deux temps :

1. `server::parser::parse_payload` reconnaît désormais les blocs DEFINE au format
   réel de la grammaire (§9) — `DEFINE <identifier> AS <entity_type> [attrs]`, une
   ligne plate, PAS un arbre `Block` — et les stocke dans `CstlPayload.defines: Vec<
   HashMap<String, String>>`, même esprit que `relations`. Aucune réintroduction
   d'`ast::Block` : c'est une extension du format à plat déjà utilisé pour RELATION,
   pas un nouveau système structurel.
2. `server::validator::check_coref_with_references(payload)` compare chaque
   `coref_with=<id>` porté par une RELATION du payload contre les `id=` collectés
   dans `payload.defines` du MÊME payload, et émet un avertissement `R8` (chaîne
   littérale, même convention que R9/R10) sur toute référence orpheline. Câblé sur
   le chemin TCP réel (`server/handler.rs`), avertissement seul — jamais un rejet,
   même politique que `check_sdl_operator_whitelist`/
   `check_extended_semantic_diagnostics`.

**Portée assumée** : intra-payload seulement, conforme au libellé de la spec §13
("un DEFINE antérieur" — dans le même document). Ne consulte PAS l'historique
cross-payload de `execution_lab::check_deontic_consistency_with_history`, qui répond
à une question différente (cohérence déontique dans le temps, pas résolution de
coréférence locale à un document). Étendre R8 à l'historique croisé (un
`coref_with` qui référence une entité DEFINE-ée dans un PAYLOAD ANTÉRIEUR plutôt que
dans le même document) reste un prolongement possible, non fait ici faute de besoin
identifié.

Tests : `server::parser::tests` (6 tests DEFINE/R7, dont malformation d'en-tête et
crochet jamais fermé avant `---END---`) et `server::validator::tests` (4 tests R8,
dont référence valide/orpheline/absence totale de DEFINE). Vérification live de bout
en bout sur un vrai `CstlNativeServer` : `examples/coref_r8_r7_smoke_test.rs`
(`cargo run --example coref_r8_r7_smoke_test`).

Note : R10 applique le théorème k=9 au niveau du validator. k=9 est la
justification formelle (croissance logarithmique avec la taille du corpus) ;
R10 est la règle d'implémentation correspondante.

---

## 20. Implémentations de référence

### 20.1 Parser Rust (production) — réécrite intégralement, 2026-09-04

**Cette section décrivait une API qui n'a JAMAIS existé dans ce dépôt** — trouvaille
de l'audit multi-angle du 2026-09-04. Aucun des points d'entrée listés ci-dessous
(`parse()`, `is_valid()`, `equivalent()`, le type `CstlDocument`, le binaire CLI
`cstl_validate`) n'a jamais été implémenté ; les modules `canonical`,
`relation_validator` et `validator_semantic` cités ont soit été des tentatives
abandonnées puis supprimées (`validator_semantic.rs`, 987 lignes, supprimé 2026-09-04
— aucun appelant réel en dehors de ses propres tests), soit n'ont jamais eu
d'appelant réel (`canonical.rs`, supprimé plus tôt cette même session). `token.rs` et
`parser.rs`/`validator.rs` (racine) ont eux aussi été supprimés le 2026-09-04 pour la
même raison (code mort, zéro appelant réel).

**Ce qui existe réellement et tourne en production** (crate `cstl_parser`, voir
`src/lib.rs` pour la liste exhaustive des modules) :

- Format wire à plat (pas d'AST/arbre `CstlDocument`) : chaque payload est parsé en
  une structure `CstlPayload` (HashMap de blocs → HashMap de clé/valeur), pas en un
  arbre de `Block`/`Field` structuré. Point d'entrée réel :
  `server::parser::parse_payload(raw: &str) -> Result<CstlPayload, ParseError>`.
  Blocs reconnus : `META`, `INTENT_PAYLOAD`, `RELATION` (depuis l'origine) et
  `DEFINE` (ajouté le 2026-09-05, pour reconstruire R8 — voir §19 — au format
  réel de la grammaire §9, une ligne plate par entité, pas un bloc `RELATIONS[...]`
  multi-lignes ni un arbre).
- Validation structurelle/format : `server::validator::validate_payload(payload:
  &CstlPayload) -> ValidationResult` (codes E301–E310, voir §16.4).
- Validation sémantique : `semantic::SemanticValidator::validate()` (codes E101,
  E107–E109, E701, W502–W605, R9, R10, voir §16.4).
- Hachage canonique et chaîne d'audit : `server::audit::canonical_hash()` /
  `HashChain` (SHA-256, via la crate `sha2` — voir la note "pas zéro dépendance"
  ci-dessous).
- Persistance : `adn_store::AdnStore` (SQLite via `rusqlite`, une seule `Connection`
  pour l'historique ADN et la chaîne d'audit depuis la fusion du 2026-09-04).
- Identité cryptographique : `signing::check_signature`/`check_rotation_signature`
  (Ed25519 via `ed25519-dalek`).
- Serveur TCP natif : `server::CstlNativeServer` (`with_data_path`/
  `try_with_data_path`, `start()`).

**"Rust pur, zéro dépendances externes" est faux** et l'a toujours été pour la couche
serveur : `tokio` (TCP async), `rusqlite` (ADN store), `sha2` (hash), `ed25519-dalek`
(signature), `serde`/`serde_json`, `unicode-normalization` sont des dépendances de
production réelles. Voir README.md section "Rust Server" pour le détail — cette
correction-là datait déjà du 2026-09-03 dans le README ; cette section de la spec ne
l'avait jamais reprise.

Aucun binaire CLI `cstl_validate` n'existe. Le seul binaire du crate est le serveur
TCP lui-même (`src/main.rs`, port 5050).

Compte de tests : voir `cargo test --lib` — ce nombre change à chaque session de
travail et une valeur figée ici divergerait rapidement (trouvaille de l'audit :
6 chiffres différents coexistaient déjà entre ce document, README.md et
docs/ARCHITECTURE.md avant cette correction). Ne pas re-figer de nombre ici.

### 20.2 Parser Python (référence) — également fictif, corrigé 2026-09-04

**Cette sous-section décrivait elle aussi une implémentation qui n'existe pas dans ce
dépôt.** Aucun `CSTLError`, aucune fonction `cstl.equivalent()`/`cstl.canonicalize()`,
aucun `SessionValidator` Python n'a été trouvé — ni dans `sdk/python/`, ni ailleurs.
Zéro fichier de test Python existe dans ce dépôt (`sdk/python/` contient
`cstl_client.py`, `cstl_llm_agent.py`, `cstl_orchestrator.py` : des clients/relais TCP
vers le serveur Rust, pas un second parser/validateur indépendant). Le nombre "201
tests" cité ici n'a jamais pu être vérifié et est retiré plutôt que corrigé, faute de
code auquel le rattacher.

---

## 21. Payload de référence — couverture complète 36 opérateurs

Ce payload couvre tous les opérateurs, tous les types DEFINE, toutes les
modalités, tous les statuts UNCERTAINTY, tous les attributs.

```
#!CSTL v5.0.0 MODE=A
META [
CONVERSATION_ID=spec_coverage_v5,
DOMAIN=medical,
PARENT_HASH=root,
TIMESTAMP=2026-06-22T00:00:00Z,
TURN=1,
encoder=Agent_CLAUDE,
produced_by=anthropic/claude-sonnet,
sigma=0.92
]
(RULE) assistant MUST respond_exclusively_in_cstl
(RULE) assistant MUST_NOT output_prose_or_explanation
(RULE) assistant MUST begin_response_with_hashbang_line
(RULE) assistant MUST copy_all_rules_to_output
(RULE) assistant MUST preserve_all_existing_relations

GRAMMAR_PRIMER [
relation=(subject) OPERATOR object [sigma=0.9, τ=present, id=rXXX],
constraint=(MUST) subject OPERATOR object [sigma=0.9, id=cXXX],
uncertainty=field ESTIMATED [sigma=0.72]
]

INTENT_PAYLOAD [
reason=clinical_audit,
sender=Agent_CLAUDE,
receiver=Agent_GPT,
purpose=full_spec_coverage,
priority=critical,
context=multi_hop_test
]

DEFINE patient AS human [id=e001, age=67, sigma=0.99]
DEFINE physician AS agent [id=e002, role=cardiologist]
DEFINE drug_A AS concept [id=e003, name=metformin, layer=bedrock]
DEFINE drug_B AS concept [id=e004, name=amlodipine, layer=deep]
DEFINE protocol AS document [id=e005, type=WHO_2026]
DEFINE monitor AS system [id=e006, type=EHR]
DEFINE risk AS threat [id=e007, type=renal_failure, layer=bedrock]
DEFINE outcome AS deliverable [id=e008, type=audit_report]
DEFINE trial AS event [id=e009, type=clinical_phase2]
DEFINE site AS place [id=e010, name=hospital_A]

CONSTRAINTS [
(MUST) physician PRESCRIBE drug_A [sigma=0.92, id=c001]
(MUST) physician PRESCRIBE drug_B [sigma=0.88, id=c002]
(MUST_NOT) patient TAKE drug_A [sigma=1.0, condition=renal_failure, id=c003]
(MAY) physician ADJUST dose [sigma=0.75, id=c004]
(SHOULD) physician MONITOR renal_function [sigma=0.90, id=c005]
(IF) risk ENTAILS contraindication_active [sigma=0.95, id=c006]
(IFF) dual_therapy EQUALS standard_protocol [sigma=0.88, id=c007]
(UNLESS) patient POSSESSES renal_failure [sigma=0.99, id=c008]
]

UNCERTAINTY [
treatment_response ESTIMATED [sigma=0.72]
drug_interaction UNKNOWN
long_term_outcome INFERRED [sigma=0.60]
biomarker_level MEASURED [sigma=0.95]
]

RELATIONS [
(patient) POSSESSES risk [sigma=0.85, τ=present, δ=bedrock, id=r001]
(drug_A) BEFORE drug_B [sigma=0.90, τ=future, id=r002]
(monitor) DURING trial [sigma=0.95, τ=present, id=r003]
(outcome) AFTER trial [sigma=0.88, τ=future, id=r004]
(physician) KNOWS diagnosis [sigma=0.97, τ=present, δ=bedrock, id=r005]
(physician) BELIEVES therapy_effective [sigma=0.78, τ=present, id=r006]
(physician) ASSUMES renal_stable [sigma=0.60, τ=past, id=r007]
(physician) DOUBTS adverse_event [sigma=0.25, τ=future, id=r008]
(therapy) ENTAILS monitoring_required [sigma=0.92, id=r009]
(drug_A) ENTAILS renal_monitoring [sigma=0.88, id=r010]
(monitoring_required) ENTAILS renal_monitoring [sigma=0.95, id=r011]
(risk) CONTRADICTS therapy_safe [sigma=0.90, id=r012]
(therapy) RESEMBLES protocol [sigma=0.85, id=r013]
(physician) CO_LOCATES monitor [sigma=0.99, τ=present, id=r014]
(drug_A) OPPOSES renal_failure [sigma=0.70, id=r015]
(therapy) COMPARES protocol [sigma=0.88, axis=efficacy, id=r016]
(protocol) TRANSMIT_FAITHFUL physician [sigma=0.95, id=r017]
(protocol) TRANSMIT_INFER physician [sigma=0.80, id=r018]
(physician) INTENT treatment_success [sigma=0.92, id=r019]
(patient) ARR therapy [sigma=0.88, τ=future, id=r020]
(patient) ARR.CREATE trial_record [sigma=0.85, id=r021]
(physician) ARR.JOIN clinical_team [sigma=0.90, id=r022]
(protocol) ARR.PRODUCE outcome [sigma=0.88, id=r023]
(monitor) ARR.ACCESS patient_record [sigma=0.99, id=r024]
(monitor) MAINTAIN patient_record [sigma=0.99, id=r025]
(risk) AMP contraindication_active [sigma=0.85, id=r026]
(risk) INH therapy_approval [sigma=0.75, id=r027]
(physician) TRANSFORM diagnosis [sigma=0.80, id=r028]
(protocol) RESIST deviation [sigma=0.88, id=r029]
(physician) COMMAND monitor [sigma=0.90, id=r030]
(physician) ASK patient [sigma=0.85, τ=past, id=r031]
(physician) STATE diagnosis [sigma=0.95, τ=present, id=r032]
(physician) PERFORM examination [sigma=0.90, τ=past, id=r033]
(physician) RECOMMEND drug_A [sigma=0.88, id=r034]
(site) CO_LOCATES trial [sigma=0.95, τ=present, id=r035]
(outcome) EQUALS audit_standard [sigma=0.80, id=r036]
(risk) PRESSURE physician [sigma=0.70, id=r037]
(evidence) CATALYZE decision [sigma=0.82, id=r038]
]

DISAGREEMENT_BLOCK [
GAP multi_hop_curve [sigma=0.93, detail=not_yet_measured]
CONCERN single_auditor [sigma=0.87, detail=author_judged_all_results]
CAUTION small_n [sigma=0.80, detail=N_10_to_N_5_exploratory_only]
]

DECISION: proceed_to_multihop_experiment [sigma=0.90]
---END---
```

**Couverture** : 38 relations couvrant les 36 opérateurs officiels (ARR×5,
INTENT, MAINTAIN, TRANSFORM, RESIST, AMP, INH, PRESSURE, CATALYZE,
TRANSMIT_FAITHFUL, TRANSMIT_INFER, COMMAND, ASK, STATE, PERFORM, RECOMMEND,
ENTAILS×3, CONTRADICTS, KNOWS, BELIEVES, ASSUMES, DOUBTS, BEFORE, AFTER,
DURING, EQUALS×2, POSSESSES, RESEMBLES, CO_LOCATES×2, OPPOSES, COMPARES),
toutes les modalités CONSTRAINTS (MUST, MUST_NOT, MAY, SHOULD, IF, IFF, UNLESS),
les 4 statuts UNCERTAINTY (ESTIMATED, UNKNOWN, INFERRED, MEASURED),
les 10 types DEFINE (human, agent, concept, document, system, threat,
deliverable, event, place — infrastructure absente intentionnellement du payload
de test médical), tous les attributs σ/τ/δ avec formes verbeuses.

Note : `therapy`, `evidence`, `diagnosis` et autres concepts apparaissant comme
objets dans RELATIONS sont des références implicites — non définies dans DEFINE
mais acceptées par le parser (loi de Postel). Seuls les 10 types officiels
nécessitent un DEFINE explicite quand l'entité est le sujet d'une relation.

---

## 22. Payload minimal de référence

```
#!CSTL v5.0.0 MODE=A
META [
PARENT_HASH=root,
encoder=Agent_CLAUDE,
produced_by=anthropic/claude-sonnet,
sigma=0.88
]
(RULE) assistant MUST respond_exclusively_in_cstl
(RULE) assistant MUST_NOT output_prose_or_explanation
CONSTRAINTS [
(MUST) sender DELIVER audit_trail [sigma=0.92]
(MUST_NOT) system PERFORM auto_decision [sigma=1.0]
]
UNCERTAINTY [
compliance INFERRED [sigma=0.70]
]
DECISION: example_decision [sigma=0.88]
---END---
```

---

## 23. Ce qui reste à faire (par priorité)

1. **Courbe multi-hop** — dégradation sémantique sur 1/2/3/5 hops (BLOCKER arXiv). **Partiellement fait le 2026-09-06** : origine du chiffre "99.3%, 12+ hops" retracée — aucun harnais automatisé produisant ce chiffre n'existe dans ce dépôt ; seuls des restes (`hop_scorer.py`, `ref_hop0.cstl`, deux fichiers hop tronqués/vides) subsistent d'une méthode manuelle décrite dans `CSTL_ARCHITECTURE_COMPLETE.pdf` (copier-coller humain dans des interfaces de chat, audit par un seul auditeur, non reproductible ici). Un nouveau harnais (`scripts/multi_hop_fidelity_curve.py`) mesure désormais, à exactement 1/2/3/5 hops : la fidélité de transport réelle contre le vrai serveur TCP compilé (100% aux quatre profondeurs, mesure réelle) et la survie du contenu sous un relais **simulé par un stub déterministe** (`DeterministicStubBackend`, PAS un LLM) — sa condition `regle_stricte` reproduit fidèlement le seul vrai résultat empirique à 1 hop (100%, règle d'immutabilité, §5) ; au-delà de hop 1, et pour `sans_regle` entièrement, les chiffres obtenus (100/100/100/100% vs 93.8/92.7/88.5/81.2% à 1/2/3/5 hops, `--runs 8`) sont une extrapolation calibrée du stub, pas une mesure d'un vrai modèle. **Reste à faire par l'utilisateur** : brancher un vrai backend LLM (`ANTHROPIC_API_KEY`/`GEMINI_API_KEY`, sur sa propre machine) à la place du stub dans `run_curve()` pour obtenir la vraie courbe. Détails et limites honnêtes : `README.md` section "Honest Limitations".
2. ~~Aligner R7~~ — fait le 2026-09-05 : crochets malformés → error (dropped + warning), câblé live dans `server/parser.rs` (voir §19)
3. ~~Implémenter R8/R9/R10~~ — fait : R9/R10 câblés depuis le 2026-09-04 (`semantic.rs`), R8 reconstruit et câblé le 2026-09-05 (`server/validator.rs::check_coref_with_references`, portée intra-payload — voir §19 pour la limite honnête)
4. ~~Mesure accord inter-opérateurs~~ — **harnais construit et vérifié structurellement le 2026-09-05** (`scripts/operator_agreement_study.py`) : 23 faits de référence (couvrant épistémique/temporel/relationnel/causalité/actes de langage/dynamique), prompt standardisé avec la liste exacte des opérateurs officiels (README §"v5.0.0 Operators"), interface de backend pluggable (`AnthropicBackend` réel avec dégradation propre si `anthropic`/`ANTHROPIC_API_KEY` absents, `DeterministicStubBackend` sans réseau), scoring d'accord brut + Cohen's kappa (2 annotateurs) / Fleiss' kappa (N≥3), rapport avec les faits les plus controversés en tête. Vérifié en direct dans ce sandbox via `pytest scripts/test_operator_agreement_study.py` (9 tests, tous passants — accord parfait entre stubs identiques, désaccord forcé sur 3 faits connus correctement détecté et remonté en tête du rapport) et `python3 scripts/operator_agreement_study.py --dry-run`. **Ajouté le 2026-09-05** : `GeminiBackend` (même interface `OperatorChoiceBackend`, `--backends anthropic,gemini`) — Gemini a un vrai palier gratuit (clé sur aistudio.google.com/apikey, aucune carte de crédit requise), contrairement à l'API Anthropic (payante, distincte d'un abonnement Claude Max qui ne couvre que le chat) — rendant une vraie comparaison inter-modèles accessible à l'utilisateur sans frais. `google-genai` (SDK unifié actuel) installé et importé avec succès dans ce sandbox ; plomberie réelle (`client.models.generate_content(...).text` → parsing JSON) vérifiée via un faux client injecté (2 nouveaux tests), dégradation propre sans clé confirmée. **Aucune mesure empirique réelle n'existe encore** — ni `ANTHROPIC_API_KEY` ni `GEMINI_API_KEY` n'étaient disponibles dans cet environnement, et aucun appel réseau réel n'a été fait ; il reste à l'utilisateur de lancer `python3 scripts/operator_agreement_study.py --backends anthropic,gemini` (après `pip install anthropic google-genai` + les deux clés) sur sa propre machine pour une vraie comparaison inter-modèles.
5. ~~Vérification déontique forte — porter cstl_verifier.py en Rust~~ — **reformulé et fait le 2026-09-05** : recherche exhaustive dans ce dépôt et dans les docs du projet (`project_search`) préalable à toute implémentation — aucun fichier `cstl_verifier.py` réel n'a été retrouvé nulle part, seulement des mentions de cet item dans cette même liste ; ce n'était donc pas un port possible. Construit directement en Rust à partir des axiomes SDL déjà documentés en §3 : `semantic.rs::check_axiom_k_entailment` (E110, voir §16.4) implémente l'axiome K (distributivité, O(φ→ψ) → (O(φ)→O(ψ))) sur des chaînes `ENTAILS` factuelles déclarées dans le payload — détecte une contradiction `MUST`/`MUST_NOT` distribuée sur deux objets différents reliés par une chaîne `ENTAILS` explicite, une classe de contradiction qu'E107 (même objet exact requis) ne voyait pas. Bloquant, comme E107, câblé sur le pipeline TCP réel (`server/validator.rs::validate_deontic_constraints`). Vérifié : 6 tests unitaires (détection directe, détection transitive à 2 sauts, non-déclenchement sujets différents, non-déclenchement sans chaîne ENTAILS, non-duplication d'E107) + scénario 5 de `examples/deontic_audit_smoke_test.rs` exécuté en direct contre le vrai serveur TCP. Les axiomes C (agglomération) et N (nécessitation des tautologies) restent non opérationnalisables sur des faits atomiques au sol (ce sont des propriétés de clôture logique, pas des contraintes vérifiables sur un ensemble plat de relations) — honnêtement non traités, pas oubliés
6. **Multi-judge** — validation indépendante des résultats empiriques
7. **Open weights** — Mistral, DeepSeek, Qwen

---

## 24. Éléments hors scope (v5.0)

- CASTLE mode réseau (modes B/C, dictionnaire partagé)
- Mode binaire
- Bloc `SELF_DECLARE`
- Mode delta-payload ADN
- Modes simulation `■` et archéologique `«`
- Recherche sémantique par embeddings dans l'ADN store

---

## 25. Bibliographie

- Allen, J.F. (1983). Maintaining knowledge about temporal intervals. *CACM*, 26(11), 832–843.
- Fagin, R., Halpern, J.Y., Moses, Y., & Vardi, M.Y. (1995). *Reasoning about Knowledge*. MIT Press.
- Feinstein, A.R., & Cicchetti, D.V. (1990). High agreement but low kappa. *J. Clinical Epidemiology*, 43(6), 543–549.
- Hintikka, J. (1962). *Knowledge and Belief*. Cornell University Press.
- McNamara, P. (2006). Deontic Logic. *Stanford Encyclopedia of Philosophy*.
- Prakken, H., & Sartor, G. (1997). Argument-based extended logic programming with defeasible priorities. *J. Applied Non-Classical Logics*, 7(1), 25–75.
- von Wright, G.H. (1951). Deontic Logic. *Mind*, 60(237), 1–15.
- Wilson, E.B. (1927). Probable inference, the law of succession, and statistical inference. *JASA*, 22(158), 209–212.

---

## 26. CHANGELOG

### v5.0.0 — révision 5 (4 septembre 2026, corrections d'audit)
- **Corrigé (critique)** : §20.1 décrivait une API Rust entièrement fictive
  (`parse()`/`is_valid()`/`equivalent()`, type `CstlDocument`, CLI `cstl_validate`,
  modules `canonical`/`relation_validator`/`validator_semantic` — aucun n'a jamais
  existé ou tous ont depuis été supprimés). Réécrite pour décrire les points d'entrée
  réels (`server::parser::parse_payload`, `server::validator::validate_payload`,
  `semantic::SemanticValidator::validate`, etc.) et retirer la fausse affirmation
  "zéro dépendances externes".
- **Corrigé (critique)** : §20.2 décrivait un parser Python (`CSTLError`,
  `SessionValidator`, "201 tests") qui n'existe pas non plus dans ce dépôt — retirée
  et remplacée par un constat honnête (`sdk/python/` ne contient que des clients TCP
  vers le serveur Rust, zéro fichier de test Python).
- **Corrigé (critique)** : §16.2 documentait `SessionValidator`/E106–E112 comme
  implémentés — jamais portés en Rust. Collision de nomenclature signalée : le moteur
  Rust réel (`semantic.rs`) réutilise E107/E108/E109 pour des vérifications
  différentes (Axiome D, contradiction temporelle, conflit AMP/INH) — voir la
  nouvelle §16.4 qui documente les codes réellement émis.
- **Corrigé** : §19 R8 marqué "spécifié" alors que son unique implémentation
  (`defined_entities`/`check_undefined_entity_reference`, sur le système `Block`
  mort) a été supprimée le même jour — passé à "retiré". R9/R10 marqués "spécifié"
  alors qu'ils sont câblés live depuis le 2026-09-04 — passés à "câblé live".
- Audit complet mené le même jour (hors périmètre normatif de ce document) :
  désynchronisation des comptes de tests entre README.md/docs/ARCHITECTURE.md/ce
  document, code mort supprimé dans `domains.rs`/`token.rs`/`arbitration_wire.rs`,
  robustesse du démarrage serveur — voir les commits Rust correspondants et
  README.md/docs/ARCHITECTURE.md pour le détail.

### v5.0.0 — révision 4 (22 juin 2026)
- Corrigé : §10.5 notation BEFORE/AFTER en notation Allen standard (end/start)
- Corrigé : §21 note de couverture — références implicites vs DEFINE explicites clarifiées

### v5.0.0 — révision 3 (22 juin 2026)
- Corrigé : §2 distinction encodeur/parser explicite (BNF prescriptive ≠ Postel)
- Corrigé : §3.2 axiome T KNOWS clarifié — cadre épistémique distinct de SDL
- Corrigé : §6 liste intent_attr exhaustive vs indicative précisée
- Ajout : §10 total 36 opérateurs récapitulé explicitement
- Corrigé : §10.1 PRESSURE, CATALYZE listés ; §21 payload couvre 38 relations / 36 opérateurs
- Corrigé : §11 `label` défini dans la BNF
- Ajout : §14 exemple concret GRAMMAR_PRIMER + COPIED_RULES
- Corrigé : §15.3 vecteurs #1 et #2 avec hash SHA-256 réel calculé
- Corrigé : §17 SEC_Q3 ajouté, numérotation Q1/Q2/Q3/Q4/Q5 contiguë
- Corrigé : §13 `coref_with` ajouté à la BNF attr_list
- Ajout : §4.2 champ DOMAIN documenté

### v5.0.0 — révision 2
- 10 corrections (PB1-PB6, BNF META, CONSTRAINTS deux formes, axiomes SDL,
  RULE BNF, R10/R13 unifiés, vecteurs round-trip, AGREEMENT/DISAGREEMENT,
  GRAMMAR_PRIMER, payload minimal)

### v4.9.3 (7 juin 2026)
- Parser Rust 112 tests, Python 201 tests
- produced_by=UNKNOWN, SessionValidator E106–E112, Security E201–E205

### v4.0 (30 avril 2026)
- INTENT_PAYLOAD, CONSTRAINTS avant RELATIONS, UNCERTAINTY, Meta-header
