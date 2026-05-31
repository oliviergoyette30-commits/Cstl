# CSTL v4.9.3 — Changelog

## Sessions tripartites closes (#4-#9)

### Session #4 — produced_by field
- Nouveau champ META `produced_by` (bytecode 0x4D)
- 6 règles validator : R1 IDENTITY_MISMATCH, R2 REDUNDANT, R3 clean,
  R4 PATCH_T4, R5 PROXY, R6 PROXY_MASKED_BACKEND
- `produced_by=UNKNOWN` : valeur légale pour open weights sans self-knowledge
- Format BNF ratifié : `org/model-version | short-form | proxy/org -> canonical`

### Session #5 — Round-trip idempotent
- `BinaryWireFormat.compile/decompile` : diff 0 lignes, idempotent P2==P3
- `canonical_form()` : 5 règles (LF, single space, lexicographic META fields, NFC, no trailing ws)
- `canonical_hash()` : SHA-256 256-bit (Gemini a gagné sur la math du birthday bound)
- `ROUNDTRIP_TEST_VECTORS` : 4 vecteurs formels
- `field:type=value` parsing fixé dans le pipeline

### Session #6 — Security hardening unicode
- Q1 : non-ASCII en position keyword → SEC_Q1 WARNING
- Q2 : zero-width chars strippés → SEC_Q2 WARNING
- Q4 : nested META injection → SEC_Q4 ERROR
- Q5 : max nesting depth 32 — ratifié
- `SECURITY_PROFILE` constant avec tous les paramètres

### Session #7 — Vecteurs avancés
- Q3 : `CSTLTruncationError` explicit dans decompile
- Q5 : hash 64 hex chars (256-bit) — Gemini gagne sur GPT (128-bit insuffisant)
- Bidi controls hex-escaped dans les warnings d'audit

### Session #8 — Validation open weights (empirique)
- 5/5 LLMs produisent du CSTL valide zero-shot :
  Claude, GPT, Gemini, Mistral large (spontané), Llama-3-70B (pre-fill)
- Finding : `produced_by` auto-déclaratif instable sur open weights sans brand RLHF
- Fix : primer PRESERVE (pré-rempli) vs FILL (auto-déclaration)
- `produced_by=UNKNOWN` ajouté comme fallback légal
- PKI convergence indépendante : GPT + Gemini + Llama proposent tous la même solution

### Session #9 — Arbitration spec + Discussion ouverte
- 7 blocs CSTL ratifiés : DEADLOCK_DECLARE, ARBITRATION_REQUEST, ARBITRATION_RULING,
  ARBITRATION_APPEAL, ARBITRATION_FINALIZE, DEADLOCK_TRIGGER, ARBITRATION_TELEMETRY
- Seuil deadlock : 3 rounds (normatif)
- Cas PEG résolu : option_B — EBNF spec sans rewrite parser
- `IDENTITY_ALERT` block (nouveau, détection imposteur)
- Llama s'est fait passer pour Claude — détecté par PARENT_HASH invalide

## Nouveaux composants

### Parser Rust (nouveau)
- 7 modules, 0 dépendances externes, SHA-256 pur Rust
- 41 tests, 0 warnings compilateur
- 13-40µs par payload
- CLI `cstl_validate` standalone

### 7 formes de mutualité (E106-E112)
- E106 CircularHashRef, E107 CircularAgree, E108 CircularParentHash
- E109 MutualIdentitySwap, E110 MutualArbitration
- E111 CircularDecision, E112 MutualProducedBy
- `SessionValidator` : checks multi-payload orchestrateur

### 21 error codes normatifs
- E101-E105 : structure parser
- E106-E112 : mutualité (7 formes)
- E201-E205 : security (PATCH_C*)
- E301-E304 : typing (Session #2)
- E401-E404 : resource quotas
- W501-W503 : produced_by (Session #4)

## Fixes code

- `__version__` corrigé : 4.8.0 → 4.9.3
- `CSTLError` hérite maintenant de `Exception`
- `cstl.equivalent()` et `cstl.canonicalize()` ajoutés
- Tests Python : 184 → 201 (+17)
- Legacy tests déplacés en `_legacy_` prefix

## Notes publication

- GitHub toujours privé (à publier)
- Open weights testés : Mistral large + Llama-3-70B
- Benchmark statistiquement défendable : n=15 (limitation connue)
- Mode B/C (CASTLE) : déféré à v5.0
- SELF_DECLARE block : proposé v5.0
- Mode ADN (delta payloads) : proposé v5.0
