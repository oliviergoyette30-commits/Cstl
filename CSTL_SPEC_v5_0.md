# CSTL v5.0.0 — Specification

**Status**: v5.0.0, 22 juin 2026  
**Author**: Olivier Goyette  
**License**: Apache 2.0  
**Supersedes**: CSTL_SPEC_v4.0.md  

---

## 1. Document grammar

```ebnf
document         ::= header (intent_payload)? (meta)?
                     (constraints)? (uncertainty)? (define_block)*
                     (relations_block)? trailer ;

header           ::= signature NEWLINE+ ;
signature        ::= "#!CSTL" SP version SP "MODE=A" ;
version          ::= "v" digit+ "." digit+ ("." digit+)? ;

trailer          ::= "---END---" ;
```

Parser MUST accept blocks in any order after the header (Postel's law).  
Encoder MUST emit blocks in canonical order shown above.

---

## 2. Formal Semantics — Deontic Logic (v5.0 addition)

### 2.1 Theoretical grounding

CSTL deontic operators are grounded in Standard Deontic Logic (SDL) as
formalized by von Wright (1951) and extended by McNamara (2006).

SDL is a modal logic system with three primitive operators:

| SDL operator | CSTL operator | Semantics |
|---|---|---|
| O(φ) — Obligatory | MUST | φ is obligatory in all accessible ideal worlds |
| P(φ) — Permitted | MAY | φ holds in at least one accessible ideal world |
| F(φ) — Forbidden | MUST_NOT | ¬φ is obligatory; equivalent to O(¬φ) |

**Kripke semantics**: A CSTL payload is interpreted over a Kripke frame
⟨W, R, V⟩ where W is a set of possible worlds, R is an accessibility
relation (ideal-world relation), and V is a valuation function.

- `MUST φ` is true at world w iff φ is true at all worlds w' accessible from w
- `MAY φ` is true at world w iff φ is true at some world w' accessible from w
- `MUST_NOT φ` is equivalent to `MUST ¬φ`

**Axioms satisfied** (following SDL):

```
(M)  MUST(φ) → φ                       -- if obligatory then true (factivity)
(C)  MUST(φ) ∧ MUST(ψ) → MUST(φ ∧ ψ)  -- agglomeration
(N)  MUST(⊤)                            -- necessity of tautologies
(D)  MUST(φ) → MAY(φ)                  -- Deontic D: obligation implies permission
```

**Honest limitation**: CSTL enforces these axioms structurally through
`sigma` values and `MUST_NOT` presence checks. It does NOT implement a
full theorem prover. A CSTL validator detects syntactic violations;
semantic modal consistency is the encoder's responsibility.

**σ mapping to force**:  
`sigma=1.0` corresponds to strict O(φ); `sigma=0.5` to weak recommendation.
This is an extension of SDL into a weighted deontic logic following the
tradition of defeasible deontic logic (Prakken & Sartor, 1997).

### 2.2 Epistemic operators (v5.0)

Grounded in epistemic modal logic (Hintikka, 1962):

| Operator | Epistemic meaning | SDL analog | Expected sigma |
|---|---|---|---|
| KNOWS | K_a(φ): agent a knows φ (factual, high certainty) | O (factual) | ≥ 0.80 |
| BELIEVES | B_a(φ): agent a believes φ (justified but fallible) | P (epistemic) | 0.4–0.85 |
| ASSUMES | A_a(φ): agent a takes φ as working hypothesis | P (provisional) | 0.3–0.75 |
| DOUBTS | D_a(φ): agent a has low confidence in φ | F (epistemic) | ≤ 0.50 |

**Key distinction**: KNOWS implies factual certainty (sigma ≥ 0.8); 
BELIEVES allows for fallibility. This mirrors the philosophical K-B distinction
in epistemic logic (Fagin et al., 1995).

Validator emits W604 when KNOWS is used with sigma < 0.8, and W605 when
DOUBTS is used with sigma > 0.5.

---

## 3. INTENT_PAYLOAD block

```ebnf
intent_payload ::= "INTENT_PAYLOAD" "[" intent_attrs "]" ;
intent_attrs   ::= intent_attr ("," SP? intent_attr)* ;
intent_attr    ::= ("priority" | "sender" | "receiver" | "purpose"
                   | "reason" | "context") "=" value ;
```

---

## 4. META block

```ebnf
meta      ::= "META" "[" meta_field ("," meta_field)* "]" ;
meta_field ::= key (":" type_hint)? "=" value ;
```

**Mandatory fields**: `encoder`, `sigma`, `RESPONSE_FORMAT`, `NO_PROSE`, `PARENT_HASH`  
**Optional**: `produced_by`, `TURN`, `TIMESTAMP`, `CONVERSATION_ID`, `ACTION`

`produced_by=UNKNOWN` is valid for open-weight models.

---

## 5. CONSTRAINTS block

```ebnf
constraints     ::= "CONSTRAINTS" "[" constraint_line* "]" ;
constraint_line ::= "(" modality ")" subject operator object
                    ("[" attr_list "]")? ;
modality        ::= "MUST" | "MUST_NOT" | "MAY" | "SHOULD" | "IF"
                  | "IFF" | "UNLESS" ;
```

---

## 6. UNCERTAINTY block

Three states with distinct epistemics:

| State | Meaning |
|---|---|
| `UNKNOWN` | Information absent, unrecoverable |
| `ESTIMATED` | Approximate value with declared confidence |
| `INFERRED` | Derived by encoder, not explicit in source |

---

## 7. RELATIONS block

```ebnf
relations_block ::= "RELATIONS" "[" relation_line* "]" ;
relation_line   ::= "(" subject ")" operator object
                    ("[" attr_list "]")? ;
```

---

## 8. Operators

### 8.1 Core operators (v4.x — unchanged)

| Category | Operators |
|---|---|
| Causality | ARR, ARR.CREATE, ARR.JOIN, ARR.PRODUCE, ARR.ACCESS |
| Intentionality | INTENT, MAINTAIN, TRANSFORM, RESIST |
| Dynamics | AMP, INH, PRESSURE, CATALYZE |
| Speech acts | COMMAND, ASK, STATE, PERFORM, RECOMMEND |
| Transmission | TRANSMIT_FAITHFUL, TRANSMIT_INFER |

### 8.2 Relational operators — MUTUAL deprecated (v5.0)

`MUTUAL` is **deprecated** as of v5.0. It encoded 6 semantically distinct
relations under one operator, of which two (POSSESSES, COMPARES) are
non-symmetric. This violates the principle of semantic non-ambiguity.

**Migration**: Replace MUTUAL with the appropriate specific operator:

| Use case | v5.0 operator | Symmetry |
|---|---|---|
| Identity / equivalence | EQUALS | Symmetric |
| Ownership / containment | POSSESSES | Asymmetric |
| Similarity / analogy | RESEMBLES | Symmetric |
| Co-location / co-occurrence | CO_LOCATES | Symmetric |
| Antagonism / opposition | OPPOSES | Symmetric |
| Explicit comparison | COMPARES | Asymmetric |

Validator emits **W601** for each MUTUAL occurrence with migration hint.
MUTUAL remains syntactically accepted for backward compatibility.

### 8.3 Logical operators (v5.0 — new)

| Operator | Meaning | Symmetry | Notes |
|---|---|---|---|
| ENTAILS | A ⊨ B: A logically entails B | Asymmetric | Transitive |
| CONTRADICTS | A ⊥ B: A and B are mutually inconsistent | Anti-symmetric | See W602 |

**ENTAILS** is transitive: if A ENTAILS B and B ENTAILS C, then A ENTAILS C
should be declared. Validator emits W603 when transitive closure is incomplete.

**CONTRADICTS** is anti-symmetric: declaring A CONTRADICTS B and B CONTRADICTS A
is redundant. Validator emits W602 for the redundant direction.

### 8.4 Epistemic operators (v5.0 — new)

| Operator | Meaning | sigma guidance |
|---|---|---|
| KNOWS | Factual certainty (K_a) | ≥ 0.80 |
| BELIEVES | Justified belief (B_a) | 0.40–0.85 |
| ASSUMES | Working hypothesis (A_a) | 0.30–0.75 |
| DOUBTS | Low confidence (D_a) | ≤ 0.50 |

### 8.5 Temporal operators — Allen interval algebra subset (v5.0 — new)

Based on Allen (1983) interval algebra, minimal subset for workflow encoding:

| Operator | Allen relation | Meaning |
|---|---|---|
| BEFORE | before (b) | A ends before B starts (strict) |
| AFTER | after (bi) | A starts after B ends (inverse of BEFORE) |
| DURING | during (d) | A is entirely contained within B |

**Consistency rule**: A BEFORE B and A AFTER B for the same pair is a
temporal contradiction. Validator emits **E701** (hard error).

Future v5.x may add: MEETS, OVERLAPS, STARTS, FINISHES (Allen 1983 complete set).

---

## 9. Validation rules

| Code | Severity | Condition |
|---|---|---|
| E101–E205 | error | Existing v4.9.3 rules |
| E701 | error | Temporal contradiction: A BEFORE B and A AFTER B declared |
| W601 | warning | MUTUAL operator used — deprecated, use specific operator |
| W602 | warning | CONTRADICTS redundant: A⊥B and B⊥A both declared |
| W603 | warning | ENTAILS transitive closure incomplete |
| W604 | warning | KNOWS with sigma < 0.8 — consider BELIEVES or ASSUMES |
| W605 | warning | DOUBTS with sigma > 0.5 — consider BELIEVES |

---

## 10. Attributes

Maximum **9 attributes per relation** (k=9 theorem, zero collisions on ≤10⁶ relations).

```ebnf
attr_list    ::= attribute ("," SP? attribute){0,8} ;
attribute    ::= "sigma=" float | "σ=" float
               | "tau=" time_val | "τ=" time_val
               | "layer=" layer_val | "id=" identifier
               | identifier "=" value ;

time_val     ::= "past" | "present" | "future"
               | "p" | "n" | "f" ;
layer_val    ::= "bedrock" | "deep" | "shallow" | "surface"
               | "b" | "d" | "s" | "su" ;
```

---

## 11. Reference implementations

- **Rust parser**: `cstl_parser` crate — 63 tests, 0 failures, 0 warnings
  (zero external dependencies)
- **Python parser**: `cstl_parser.py` — 201 tests, 0 failures

### 11.1 New modules in v5.0

- `relation_validator.rs` — W601–W605, E701 validation
- `token.rs` updated — 15 new keywords (ENTAILS, CONTRADICTS, BELIEVES,
  KNOWS, ASSUMES, DOUBTS, BEFORE, AFTER, DURING, EQUALS, POSSESSES,
  RESEMBLES, CO_LOCATES, OPPOSES, COMPARES)

---

## 12. Bibliography

- Allen, J.F. (1983). Maintaining knowledge about temporal intervals. *Communications of the ACM*, 26(11), 832–843.
- Fagin, R., Halpern, J.Y., Moses, Y., & Vardi, M.Y. (1995). *Reasoning about Knowledge*. MIT Press.
- Hintikka, J. (1962). *Knowledge and Belief*. Cornell University Press.
- McNamara, P. (2006). Deontic Logic. *Stanford Encyclopedia of Philosophy*.
- Prakken, H., & Sartor, G. (1997). Argument-based extended logic programming with defeasible priorities. *Journal of Applied Non-Classical Logics*, 7(1), 25–75.
- von Wright, G.H. (1951). Deontic Logic. *Mind*, 60(237), 1–15.

---

## 13. CHANGELOG v4.9.3 → v5.0.0

### Added
- **Formal semantics section** (§2): SDL axioms M, C, N, D with Kripke semantics;
  honest limitation on theorem proving scope
- **Epistemic operators** (§8.4): BELIEVES, KNOWS, ASSUMES, DOUBTS with
  sigma calibration guidelines and W604/W605 validators
- **Logical operators** (§8.3): ENTAILS (transitive), CONTRADICTS (anti-symmetric)
  with W602/W603 validators
- **Temporal operators** (§8.5): BEFORE, AFTER, DURING (Allen 1983 subset)
  with E701 contradiction detector
- **6 relational operators** (§8.2): EQUALS, POSSESSES, RESEMBLES, CO_LOCATES,
  OPPOSES, COMPARES replacing MUTUAL
- **W601**: MUTUAL deprecation warning with migration hints

### Changed
- MUTUAL marked DEPRECATED (backward compatible — warning only, not error)
- Version bump 4.9.3 → 5.0.0

### Backward compatibility
- All v4.9.3 payloads parse without errors under v5.0
- MUTUAL emits W601 but does not fail validation
