# CSTL — Compressed Semantic Transfer Language

**v4.9.3** | [![CI](https://github.com/oliviergoyette30-commits/Cstl/actions/workflows/ci.yml/badge.svg)](https://github.com/oliviergoyette30-commits/Cstl/actions) | [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> A deterministic semantic protocol for structured communication between Large Language Models.

---

## What is CSTL?

CSTL is a compact, human-readable encoding format that allows LLMs of different architectures to exchange **causal and semantic structures** with deterministic fidelity — without relying on natural language ambiguity or external tooling.

Unlike JSON, RDF, or plain text, CSTL embeds epistemic metadata (confidence `sigma`, temporality `tau`, deontic constraints) **natively in the payload**, enabling multi-agent systems to reason about uncertainty, obligations, and causality in a single pass.

### Core differentiators

| Feature | CSTL | JSON | AMR | RDF/OWL |
|---|---|---|---|---|
| Native uncertainty (`sigma`) | ✅ | ❌ | ❌ | ❌ |
| Deontic constraints (`MUST/MUST_NOT`) | ✅ | ❌ | ❌ | Partial |
| Temporal operators (`tau`) | ✅ | ❌ | Partial | Partial |
| LLM zero-shot decodable | ✅ | ✅ | ❌ | ❌ |
| Cross-model validated | ✅ (5 LLMs) | — | — | — |
| Human readable | ✅ | ✅ | ❌ | ❌ |

---

## Quick start

### Rust (recommended)

```bash
git clone https://github.com/oliviergoyette30-commits/Cstl.git
cd Cstl
cargo test        # 92 tests, 0 failures
cargo build --release
```

### Python

```bash
pip install cstl          # coming soon — PyPI release in progress
# or from source:
pip install -e .
python -m pytest          # 201 tests
```

---

## Example payload

```
#!CSTL v4.9.3 MODE=A
META [encoder=AgentA, sigma:float=0.95, TURN:int=1]
DOMAIN: medical_triage

DEFINE patient_X AS entity [age=45, symptoms=chest_pain+dyspnea]

RELATIONS [
  (patient_X) REQUIRES immediate_ecg [sigma=0.97, tau=present]
  (patient_X) MAY_HAVE myocardial_infarction [sigma=0.72, tau=present]
]

CONSTRAINTS [
  (MUST) clinician PERFORM ecg_within_10min [sigma=0.99]
  (MUST_NOT) system DELAY triage [sigma=1.0]
]
---END---
```

A second LLM receiving this payload can immediately extract: the entity, the causal relations, confidence levels, and the deontic obligations — **without any shared training or prior context**.

---

## Architecture

CSTL uses a three-layer pipeline:

```
Layer 1 — Semantic     65 primitive symbols (relations, operators, modalities)
Layer 2 — Syntax      121 structural tokens (blocks, qualifiers, types)
Layer 3 — ADN          k=9 deterministic routing theorem
```

The **k=9 theorem** is the core formal result: any CSTL relation graph with branching factor k≤9 has a unique canonical traversal order, guaranteeing deterministic reconstruction across heterogeneous LLM decoders.

---

## Validation

### Cross-model zero-shot decoding (5 LLMs)

| Model | Zero-shot decode score |
|---|---|
| Claude 3.5 Sonnet | 0.97 |
| GPT-4o | 0.95 |
| Gemini 1.5 Pro | 0.91 |
| Mistral Large | 0.88 |
| LLaMA 3 70B | 0.84 |

### Rust parser — test suite

- **92 tests**, 0 failures, 0 warnings
- Platforms: Android (CxxDroid), macOS (M1), Linux (CI Ubuntu)
- Parse time: ~115 µs per payload
- Zero external dependencies

### Python parser — test suite

- **201 tests** across 25 domains, 21 error codes, 7 mutuality forms

---

## Supported domains (25)

`medical_triage` · `legal_analysis` · `financial_risk` · `supply_chain` · `autonomous_vehicles` · `multi_agent_negotiation` · `scientific_research` · `regulatory_compliance` · `cybersecurity` · `educational_assessment` · `environmental_monitoring` · `crisis_management` · `pharmaceutical` · `urban_planning` · `military_logistics` · `insurance` · `judicial_reasoning` · `public_health` · `energy_grid` · `aerospace` · `social_welfare` · `manufacturing` · `telecommunications` · `agriculture` · `developpement_logiciel`

---

## Repository structure

```
Cstl/
├── src/                   # Rust parser (v4.9.3)
│   └── lib.rs
├── tests/                 # 92 Rust unit tests
├── cstl_codec.py          # Python codec (630 lines, PPM-C compression)
├── cstl_parser.py         # Python parser
├── tests_python/          # 201 Python tests
├── spec/                  # CSTL v4.9.3 formal specification
├── benchmarks/            # STS-B, anti-cheating Korthax domain tests
├── examples/              # Sample payloads across domains
└── .github/workflows/     # CI (Ubuntu + macOS runners)
```

---

## Benchmarks

| Metric | Value |
|---|---|
| STS-B Pearson r | 0.834 |
| STS-B Spearman ρ | 0.860 |
| Compression vs raw text | 1.45× (gzip-neutral) |
| Anti-cheating Korthax score | 100% (ChatGPT, Gemini) |
| ADN k=9 canonical fidelity | 99.9% |

> **Note on compression**: CSTL is optimized for semantic density, not byte compression. The 1.45× ratio vs raw prose reflects structural overhead that is offset by the elimination of disambiguation roundtrips in multi-agent pipelines.

---

## Comparison vs related formats

| | CSTL | JSON-LD | AMR | Knowledge Graph | Protobuf |
|---|---|---|---|---|---|
| LLM-native | ✅ | Partial | ❌ | ❌ | ❌ |
| Uncertainty modeling | ✅ | ❌ | ❌ | ❌ | ❌ |
| Deontic logic | ✅ | ❌ | ❌ | ❌ | ❌ |
| Zero external deps | ✅ | ❌ | ❌ | ❌ | ❌ |
| Human readable | ✅ | Partial | ❌ | ❌ | ❌ |
| Cross-LLM validated | ✅ | — | — | — | — |

---

## Roadmap

- [ ] PyPI publication (`pip install cstl`)
- [ ] Python round-trip fix (encoder↔decoder parity)
- [ ] Multi-hop degradation benchmark (arXiv requirement)
- [ ] arXiv preprint — cs.CL + cs.AI + cs.MA
- [ ] CSTL-Fusion operators (v3.1 extension: `⊗[n]`, `⚡`, `{cs:}`)
- [ ] Windows CI runner

---

## Contributing

Issues and PRs welcome. For protocol extension proposals, open a discussion with a minimal CSTL payload demonstrating the use case.

---

## License

MIT © Olivier Goyette

---

## Citation

If you use CSTL in research, please cite:

```bibtex
@misc{goyette2026cstl,
  title  = {CSTL: Compressed Semantic Transfer Language for Deterministic
             Inter-LLM Communication},
  author = {Goyette, Olivier},
  year   = {2026},
  note   = {Preprint in preparation. \url{https://github.com/oliviergoyette30-commits/Cstl}}
}
```
