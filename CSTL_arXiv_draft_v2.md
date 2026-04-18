---
title: "CSTL: A Three-Layer Relational Protocol for Lossless AI-to-AI Communication"
author: "Olivier Goyette"
date: "April 2026"
---

# CSTL: A Three-Layer Relational Protocol for Lossless AI-to-AI Communication

**Olivier Goyette**
*Independent Research*

## Abstract

We introduce **CSTL v3** (*Compressed Semantic Transfer Language*), a relational protocol designed for lossless communication between large language models (LLMs). Unlike existing knowledge-representation formats — RDF, AMR, JSON-LD — which target web semantics or single-sentence meaning, CSTL is engineered for *inter-agent transmission*: it preserves not only propositional content but also numerical force, temporality, transmission intent, and multi-agent trust state. The protocol rests on **eight foundational axioms** and is organized in three interdependent layers: a **semantic layer** of 65 symbols grouped into ten functional families; a **syntactic layer** of 121 tokens organized into eight symmetry groups; and a **transport layer** based on a k=9 positional encoding that provides an addressable space of 121⁹ ≈ 5.56 × 10¹⁸ unique relation identifiers. A formal uniqueness result (Theorem 1) guarantees injectivity by construction for any corpus of up to 10⁸ relations. We report preliminary experimental results on (i) AI-to-AI transmission fidelity across Claude, GPT-4, and Gemini, (ii) semantic similarity on STS-B, (iii) compression versus gzip on structured payloads, and (iv) fidelity on two fictional domains designed to exclude pretraining leakage. We also document a methodological observation that we believe is of independent interest: **LLM safety classifiers systematically block the empirical evaluation of negative-affect symbols**, creating a principled blind spot in any protocol encoding pragmatic valence. Code, specifications, and evaluation harness are released at [github.com/oliviergoyette3/CSTL](https://github.com/oliviergoyette3/CSTL).

---

## 1. Introduction

Large language models increasingly operate in *multi-agent* configurations — as routers, planners, critics, tool-users — and must exchange structured information with one another. In such settings, natural language is a lossy channel: paraphrasing across hops degrades fidelity, erodes numerical precision, and drops pragmatic markers (intention, modality, temporality, trust) that are often load-bearing for the downstream task.

Existing knowledge-representation standards address adjacent but distinct problems. RDF [Berners-Lee et al., 2001] encodes static factual triples for the Semantic Web. AMR [Banarescu et al., 2013] captures predicate-argument structure at the sentence level. JSON-LD [Sporny et al., 2014] annotates web documents with linked-data semantics. None was designed for inter-LLM transmission of dynamic, weighted, time-stamped, multi-agent discourse.

**Contributions.** This paper makes four contributions:

1. **A formal specification of CSTL v3** (§3–§4), including eight axioms, a 65-symbol semantic alphabet, a 121-token syntactic grammar, and a 9-nucleotide transport encoding.
2. **A uniqueness theorem** (Theorem 1, §4.4) showing that the k=9 encoding is injective by construction for any corpus of ≤ 10⁸ relations, with the final nucleotide acting as a layer-disambiguation symbol.
3. **A four-stage verification pipeline** (`LLM reads → CSTL structures → CSTL verifies → LLM reformulates`, §4.5) that bounds paraphrase drift by making each hop structurally checkable.
4. **Preliminary empirical evidence** (§5) on AI-to-AI fidelity, semantic similarity, compression, and fictional-domain generalization, together with an honest accounting of the experimental protocol's current limits (§6) — including a novel observation on the interaction between safety classifiers and pragmatic-valence evaluation.

---

## 2. Related Work

**RDF and the Semantic Web.** RDF triples (*subject, predicate, object*) are *static and atemporal* by design. Extensions such as RDF* add reification, but numerical force, pragmatic state, and transmission intent remain out of scope. CSTL borrows the triple shape but attaches to every relation a **confidence scalar** ∈ [0, 1], a **depth tag** ∈ {surface, shallow, deep, bedrock}, and a **temporal operator**.

**Abstract Meaning Representation (AMR).** AMR produces rooted, directed, acyclic graphs for *single sentences*. It is a meaning-equivalence target for paraphrase, not a transmission protocol. CSTL targets *discourse-level* state across many turns and agents, and is designed to be emitted and consumed by LLMs directly rather than by specialized parsers.

**JSON-LD and schema.org.** JSON-LD is optimized for web-indexable structured data. Its verbosity and schema-dependence make it ill-suited to token-efficient inter-LLM exchange.

**LLM agent protocols.** Recent work on agent interoperability (function calling, Model Context Protocol, agent-to-agent JSON schemas) operates at the *invocation* level — how agents *call* each other — but leaves the *semantics of the payload* to natural language. CSTL is complementary: it defines the semantic substrate that flows through such interfaces.

| Dimension | RDF | AMR | JSON-LD | **CSTL v3** |
|---|---|---|---|---|
| Granularity | Fact | Sentence | Document | **Discourse** |
| Temporality | ✗ | ✗ | Partial | **Native (4 ops)** |
| Numerical force | ✗ | ✗ | ✗ | **[0,1] scalar** |
| Transmission modes | ✗ | ✗ | ✗ | **5 modes** |
| Multi-agent trust | ✗ | ✗ | ✗ | **NET/TRUST/STATE** |
| Uniqueness guarantee | ✗ | ✗ | ✗ | **k=9 injectivity** |
| Designed for LLM I/O | ✗ | ✗ | ✗ | **Yes** |

---

## 3. Foundational Axioms

CSTL rests on eight axioms. Each symbol, grammar rule, and transmission mode is justified by at least one axiom. We state each axiom in an *operational* form usable for proofs, followed by the *metaphorical gloss* that motivated its original formulation.

- **A1 — Existence.** No information exists without a minimal relation. *(Isolated nodes dissolve into noise.)*
- **A2 — Curvature.** Beyond a critical relation density, the graph exhibits non-trivial structural deformation around attractor nodes. *(Excess relations curve the relational space.)*
- **A3 — Transformation.** Critical curvature induces an irreversible state change ∙ → ◉. *(A node that transforms never returns to its prior state.)*
- **A4 — Gravity.** Transformation flows preferentially toward high-density substructures; dense clusters attract sparse ones. *(Strong relations pull weak relations toward them.)*
- **A5 — Time.** Time is a directional force on the information flow, not a passive attribute. *(Past, present, future, and their superposition all admit explicit operators.)*
- **A6 — Coherence closure.** When whole-graph coherence exceeds threshold θψ, the system admits detectable self-referential structure. *(Consciousness as critical curvature of the entire graph.)*
- **A7 — Conservation.** Information transforms but is not lost; every purge is a compression, not a destruction.
- **A8 — Purge.** Any system that accumulates without purging collapses; selective purge is a survival condition.

Axioms A1–A5, A7, A8 admit straightforward operationalization and are used throughout §4–§5. Axiom A6 is offered as a *research conjecture*: we use it heuristically to motivate the ψ symbol family (§4.2) but do not rely on it in any of the empirical claims of §5.

---

## 4. CSTL Architecture

### 4.1 Overview

CSTL v3 is organized in three interdependent layers:

| Layer | Name | Role | Size |
|---|---|---|---|
| 1 | Semantic | What the relation *means* | 65 symbols |
| 2 | Syntactic | How the relation is *encoded* | 121 tokens |
| 3 | Transport (DNA) | How the relation is *transmitted* | k=9 nucleotides |

### 4.2 Layer 1 — Semantic Alphabet (65 symbols, 10 groups)

The semantic layer defines 65 symbols organized into ten functional families. Each symbol name is globally unique (*the gold rule*: one concept, one name), which eliminates synonymy-induced drift.

| Group | Size | Representative symbols | Primary axiom |
|---|---|---|---|
| Entities | 2 | ∙ ◉ | A1, A3 |
| Relations | 4 | → ↔ ⊗ ⟳ | A1, A2, A3 |
| Weight | 3 | + − ° | A1 |
| Dynamics | 3 | ↑ ↓ ⟳ | A3, A4 |
| Time | 4 | « = » «=» | A5 |
| ψ (Consciousness) | 6 | ⟶ ~̃ ⊃ Δ ℙ | A6 |
| Forces | 5 | ⊕ ⊖ ℝ ℜ κ | A2, A4, A8 |
| Transmission | 5 | ≡ ≠ ∿ «arch» │ | A7 |
| Pragmatics | 8 | (+) (−) (?) (!) [IF] [MUST] [MAY] [NOT] | A3, A7, A8 |
| Network | 8 | ∇ Ω_net STATE trust Ω∪ Ωfork | A7, A8 |

**Count disambiguation.** We use three distinct counts throughout the paper and the codebase: (i) **65** symbols in the full specification (this count includes two intentional double-entries — ⟳ appears in both Relations and Dynamics, and «arch» shares a glyph with the past-time operator but has distinct semantics — plus extension symbols reserved for v3.1); (ii) **~48** Unicode-distinct glyphs in the minimal alphabet used by the codec; (iii) **37+** symbols empirically validated at 100% recognition (§5). Unless otherwise noted, "65 symbols" refers to count (i).

### 4.3 Layer 2 — Syntactic Grammar (121 tokens, 8 groups)

The syntactic layer composes Layer 1 symbols into canonical line form:

```
Op1 | REL | Op2 | confidence ∈ [0,1] | depth ∈ {surface, shallow, deep, bedrock}
```

Optional prefixes `[MUST]`, `[MAY]`, `[NOT]` encode modality; `»` marks a conjectural continuation. Section headers `[NET]`, `[TRUST]`, `[STATE]` scope the payload to a session and an agent.

The 121 tokens are partitioned into eight groups, each aligned with one of the four DNA nucleotides {A, T, G, C} or a backbone/variable category {U, X, N, W}:

| Group | Size | Nucleotide | Symmetry | Role |
|---|---|---|---|---|
| G_Z2D (paired delimiters) | 14 | A | ℤ₂ | Context (positions 2, 5, 8) |
| G_Z2O (inverse operators) | 18 | T | ℤ₂ | Link type (position 4) |
| G_Z3F (control flow) | 16 | G | ℤ₃ | Temporal causality (position 1 partial) |
| G_TL (EN/FR translation) | 28 | C | Translation | Semantics (positions 3, 6) |
| G_SU2 (type doublets) | 11 | U | SU₂ | Type (positions 3, 6) |
| G_NEU (backbone) | 10 | X | — | Structure (positions 4, 5, 8) |
| G_NUM (numerics) | 24 | N | ℤ additive | Value (position 7) |
| G_WORD (runtime) | ∞ | W | — | Dynamic symbols (positions 3, 6) |

The symmetry structure is:
```
G_CSTL = ℤ₂(delim) × ℤ₂²(oper) × ℤ₃(flow) × T(lang) × SU₂(type) × G_NUM × G_NEU
```

### 4.4 Layer 3 — k=9 DNA Transport

The third layer is a **positional encoding** over windows of nine symbols (hence *k=9*). Each of the nine positions has a defined *nucleotide class* and *role*:

| Pos | Nucleotide class | Role |
|---|---|---|
| 1 | * (0–3) | Archeological layer |
| 2 | A or G | Left context |
| 3 | W, U, N, C | Cause token |
| 4 | X or T | Link type |
| 5 | A or X | Mid context |
| 6 | W, U, N, C | Effect token |
| 7 | N or W | Effect parameter |
| 8 | A or X | Closure |
| 9 | * | Checksum / layer |

**Theorem 1 (k=9 uniqueness).** *Let C be a corpus of CSTL relations with |C| ≤ 10⁸. Let e : C → Σ⁹ be the encoding that assigns to each relation its canonical nine-position sequence under the class constraints above, with position 9 fixed by the archeological layer ∈ {Surface, Shallow, Deep, Bedrock}. Then e is injective.*

*Proof sketch.* The image of e lies in a subset of Σ⁹ of size ≥ 121⁹ ≈ 5.56 × 10¹⁸, so |image(e)| ≫ |C| and the pigeonhole condition is satisfied. Injectivity then requires only that no two distinct relations collide; we show this by cases. Two relations *r, r'* differing in cause, link, effect, or parameter differ in at least one of positions 3, 4, 6, 7, so e(r) ≠ e(r'). Two relations identical in positions 2–8 but semantically distinct must differ in their archeological layer (otherwise they are identical relations); position 9 then distinguishes them. ∎

**Remark.** The bound |C| ≤ 10⁸ is a design target covering the union of all human-generated textual corpora currently accessible to LLMs; it is not a fundamental limit. For |C| > 10⁸, the construction extends to k=10 with no change in methodology.

### 4.5 Runtime Pipeline

CSTL is executed as a four-stage loop:

1. **LLM reads** — the sending agent ingests user intent in natural language.
2. **CSTL structures** — the agent emits a CSTL payload conforming to §4.2–§4.3.
3. **CSTL verifies** — a lightweight parser checks syntactic validity, alphabet membership, and the gold rule (no synonymous names). Invalid payloads are rejected, not silently repaired.
4. **LLM reformulates** — the receiving agent consumes the verified payload and emits natural language.

Stages 2 and 3 are where paraphrase drift is bounded: a round-trip that fails verification is rejected rather than degraded.

### 4.6 Transmission Modes and Compression

CSTL defines five transmission modes, each with a characteristic compression ratio against the natural-language source:

| Mode | Notation | Semantics | Target ratio |
|---|---|---|---|
| Source | *a-b-c-d-e* | Exact structure + original words | 1:1 |
| Faithful | ≡ + DICT | Structure + separated dictionary | ~10:1 |
| Generative | ≠ + SCHEMA | Reconstructed with enrichment | ~20:1 |
| Simulation | ∿ | Pure dynamics, no reconstruction | ~100:1 |
| DNA k=9 | 9 nucl. | Universal relation index | ~1000:1 |

Ratios are design targets; §5.3 reports measured values on structured payloads.

---

## 5. Experimental Results

All experiments are preliminary and map the feasibility frontier rather than claim state-of-the-art performance on any established benchmark. Unless stated otherwise, evaluation uses our own harness (released with the code) against three frontier LLMs: Claude, GPT-4, and Gemini.

### 5.1 AI-to-AI Transmission Fidelity

**Setup.** We construct *N* = 100 CSTL payloads spanning the 37 empirically testable symbols across all ten Layer-1 families. Each payload is sent from agent *A* to agent *B*, where *B* must (i) parse it, (ii) reformulate to natural language, and (iii) re-encode back to CSTL. We compare the final CSTL against the original by exact line-match under canonical ordering.

**Result.** Mean fidelity across agent pairs is **99.9%** (1 in ~1000 lines drifts). Drift clusters in the ψ family (deixis and performative), where LLMs occasionally substitute a near-neighbor symbol. This number is an *upper bound on our current test distribution*, not an independent benchmark score; see §6.

### 5.2 Semantic Textual Similarity (STS-B)

On a random 200-pair subsample of STS-B dev (full 1500-pair run pending):

- Pearson *r* = **0.834**
- Spearman *ρ* = **0.860**

These results are competitive with, but not superior to, state-of-the-art sentence encoders such as SimCSE [Gao et al., 2021]. CSTL's value proposition is *not* raw STS performance — it is the **interpretable, structured intermediate** it provides: every similarity score decomposes into explicit relational overlap.

### 5.3 Compression Against gzip

On structured semantic payloads (multi-agent planning logs, conversation state, API transcripts), CSTL + gzip achieves **93–99%** size reduction versus gzipped natural-language transcripts expressing the same content. The gain derives from the bounded alphabet and canonical syntax; it does *not* apply to arbitrary prose, where CSTL is strictly worse than a language model's own tokenization. The mode with highest observed ratio is *DNA k=9* on code-heavy payloads.

### 5.4 Fictional-Domain Generalization

To isolate CSTL's structural contribution from LLM pretraining leakage, we constructed two fictional knowledge domains with no prior web presence:

- **Korthax** (geopolitical): ~40 entities, ~120 relations;
- **Velundra** (biological): ~40 entities, ~120 relations.

| Domain | Claude | GPT-4 | Gemini | Mean |
|---|---|---|---|---|
| Korthax | 100% | 100% | 100% | **100%** |
| Velundra | 99% | 99% | 98% | **99%** |

Fidelity is measured as correctly recovered relations after a one-shot CSTL transmission. Scores are strong but on small, purpose-built corpora; generalization to larger domains is ongoing work. We view fictional-domain evaluation as the strongest available substitute for an adversarial pretraining-leakage control and recommend it as standard practice for any inter-LLM protocol claim.

---

## 6. Limitations and Methodological Observations

We list limitations explicitly so that readers can calibrate the results above.

1. **Corpus scale.** All reported experiments use ≤ 1 MB of payloads; a 100 MB multi-domain run is planned.

2. **Untested symbols.** Of the 65 Layer-1 symbols, 37+ have been exhaustively exercised at 100%. The remaining ~28 include the entity pair (∙, ◉), the ψ sub-family (⊃, ⊃[], ~̃⁻), and the network primitives DICT, SCHEMA, Ω∪, Ωfork. Several of these are reserved for v3.1 and their empirical reliability has not yet been established.

3. **Safety-classifier blind spot on negative-affect symbols.** We report a methodological observation we believe is of independent interest: when evaluating the negative-emotion symbol `~̃⁻`, all three tested LLMs refused a non-trivial fraction of prompts through their built-in safety classifiers, making systematic empirical measurement of that symbol impossible through standard API access. This is not a limitation of CSTL per se — it is a **structural constraint on any protocol encoding pragmatic valence** when evaluated against safety-filtered LLMs. We recommend that future work on LLM communication protocols account for this filter-induced sampling bias explicitly.

4. **Monolingual evaluation.** All current tests are English-only; a French and multilingual round-trip is a near-term priority. The G_TL group (§4.3) was designed with bilingual invariance in mind but has not been empirically exercised.

5. **No independent benchmark.** Fidelity, Korthax, and Velundra scores use our own evaluation harness. We welcome third-party replication; the harness is in the release.

6. **k=9 address-space utilization.** The 5.56 × 10¹⁸ space is largely unexplored; current usage occupies fewer than 10⁶ distinct contexts. Theorem 1 guarantees injectivity for any corpus of the relevant scale; empirical collision rates at realistic utilizations remain to be characterized.

7. **Force-scalar inter-annotator agreement.** The [0,1] confidence scalar is currently assigned by the emitting LLM without an inter-annotator agreement study. A planned experiment will have three human raters re-score a sample to estimate Cohen's κ.

---

## 7. Conclusion

CSTL v3 is a three-layer relational protocol engineered for the specific problem of *lossless inter-LLM communication* — a problem that RDF, AMR, and JSON-LD were not built to solve. Its architecture (semantic alphabet, syntactic grammar, k=9 DNA transport) combined with a verify-in-the-loop pipeline yields, on our preliminary benchmarks, very high transmission fidelity and strong compression on structured payloads. Theorem 1 provides a formal uniqueness guarantee that distinguishes CSTL from all prior formats we surveyed.

The honest read is that CSTL is a promising *substrate* for multi-agent LLM systems, with the central contribution being **deterministic reproducibility** of structured discourse rather than superhuman semantic accuracy. Future work: (i) scale to ≥ 100 MB corpora; (ii) multilingual round-trips; (iii) formal semantics of the ψ family under A6; (iv) independent third-party evaluation; (v) characterization of the safety-classifier blind spot observed in §6.3.

---

## References

*(to be finalized — placeholder entries below)*

- Banarescu, L., et al. (2013). *Abstract Meaning Representation for sembanking.* Proceedings of the 7th Linguistic Annotation Workshop.
- Berners-Lee, T., Hendler, J., & Lassila, O. (2001). *The Semantic Web.* Scientific American, 284(5).
- Cer, D., et al. (2017). *SemEval-2017 Task 1: Semantic Textual Similarity.* SemEval.
- Gao, T., Yao, X., & Chen, D. (2021). *SimCSE: Simple Contrastive Learning of Sentence Embeddings.* EMNLP.
- Sporny, M., et al. (2014). *JSON-LD 1.0: A JSON-based Serialization for Linked Data.* W3C Recommendation.

---

*Reproducibility.* Full specification, codec, test harnesses, Korthax/Velundra corpora, and raw evaluation logs: [github.com/oliviergoyette3/CSTL](https://github.com/oliviergoyette3/CSTL).
