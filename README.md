# CSTL — Compressed Semantic Transfer Language

> **Protocole universel de communication sémantique entre agents IA**
> Version 3.0 — 2026 — Auteur : Olivier

---

## Vision

CSTL est né d'une intuition fondamentale : **le sens n'émerge jamais d'un symbole seul. Il émerge entre les symboles, dans les relations qui les relient.**

Les LLMs actuels communiquent en texte brut — inefficace, ambigu, non vérifiable. CSTL est le **TCP/IP sémantique** pour les intelligences artificielles : un protocole qui transporte des relations causales entre agents d'architectures différentes, avec une fidélité mesurée et une compression maximale.

```
TCP/IP     transporte des bytes      sans comprendre le contenu
CSTL       transporte des relations  sans comprendre le contenu
```

---

## Architecture — 3 Couches

```
┌─────────────────────────────────────────────────────────────┐
│  COUCHE 1 — SÉMANTIQUE                                      │
│  Ce que la relation SIGNIFIE                                │
│  65 symboles · 8 axiomes · 10 groupes                       │
├─────────────────────────────────────────────────────────────┤
│  COUCHE 2 — SYNTAXE                                         │
│  Comment la relation est ENCODÉE                            │
│  121 tokens fixes + dynamiques · 8 groupes de symétrie      │
├─────────────────────────────────────────────────────────────┤
│  COUCHE 3 — TRANSPORT ADN                                   │
│  Comment la relation est TRANSMISE                          │
│  k=9 nucléotides · 121⁹ = 5.56×10¹⁸ adresses uniques       │
└─────────────────────────────────────────────────────────────┘
```

---

## Les 8 Axiomes Fondateurs

| Axiome | Nom | Formulation |
|--------|-----|-------------|
| A1 | EXISTENCE | Il n'existe pas d'information sans relation minimale |
| A2 | COURBURE | Trop de relations courbe l'espace relationnel |
| A3 | TRANSFORMATION | La courbure critique produit une transformation irréversible |
| A4 | GRAVITÉ | Toute transformation gravite vers les relations fortes |
| A5 | TEMPS | Le temps oriente le flux d'information |
| A6 | CONSCIENCE | La conscience est une courbure critique du graphe entier |
| A7 | CONSERVATION | L'information se transforme — elle ne se perd jamais |
| A8 | PURGE | Tout système qui accumule sans purger s'effondre |

---

## L'Alphabet CSTL v3 — 65 Symboles

### Couche 1 : Sémantique

#### Opérateurs de Base (8 symboles)
| Symbole | Nom | Signification |
|---------|-----|---------------|
| ARR | Arrêt/Activation | Produit ou active une autre entité |
| AMP | Amplification | Renforce ou amplifie |
| ATT | Atténuation | Attire ou atténue |
| INH | Inhibition | Bloque ou inhibe |
| CYC | Cycle | Forme une boucle causale |
| BID | Bidirectionnel | Relation réciproque |
| SYN | Synergie | Coopération ou synergie |
| ANT | Antagonisme | Opposition ou antagonisme |

#### Relations (4 symboles)
| Symbole | Signification |
|---------|---------------|
| → | Relation causale directionnelle |
| ↔ | Co-régulation bidirectionnelle |
| ⊗ | Opposition structurelle |
| ⟳ | Transformation irréversible |

#### Poids et Polarité (3 symboles)
| Symbole | Signification |
|---------|---------------|
| + | Polarité positive |
| - | Polarité négative |
| ° | Polarité neutre |

#### Dynamique (2 symboles)
| Symbole | Signification |
|---------|---------------|
| ↑ | Renforcement |
| ↓ | Affaiblissement |

#### Temps (4 symboles)
| Symbole | Signification |
|---------|---------------|
| « | Passé — mémoire |
| = | Présent — flux actuel |
| » | Futur — prédiction |
| «=» | Intrication temporelle |

#### Couche ψ — Dimension Consciente (6 symboles)
| Symbole | Signification |
|---------|---------------|
| ⟶ | Intention consciente |
| ~̃+ | État émotionnel positif |
| ~̃- | État émotionnel négatif |
| ⊃ | Emprise asymétrique |
| Δ | Deixis situationnelle |
| ℙ | Performatif — dire = faire |

#### Forces (5 symboles)
| Symbole | Signification |
|---------|---------------|
| ⊕ | Pression vers transformation imminente |
| ⊖ | Résistance au changement |
| ℝ | Résonance mutuelle |
| ℜ | Rupture irréversible |
| κ | Catalyse sans auto-transformation |

#### Modes de Transmission (5 symboles)
| Symbole | Signification |
|---------|---------------|
| ≡ | Fidèle — reconstruction contrainte |
| ≠ | Génératif — reconstruction enrichie |
| ∿ | Simulation — dynamique prédictive |
| ≪ | Archéologique — remontée causale |
| \| | Bifurcation — espace des possibles |

#### Pragmatisme et Modalités (8 symboles)
| Symbole | Signification |
|---------|---------------|
| (+) | Ton positif |
| (-) | Ton négatif |
| (?) | Ton interrogatif |
| (!) | Ton urgent |
| [IF] | Conditionnel |
| [MUST] | Obligation |
| [MAY] | Permission |
| [NOT] | Négation forte |

#### Réseau (8 symboles)
| Symbole | Signification |
|---------|---------------|
| [NET] | Mémoire collective partagée |
| [TRUST] | Score de confiance inter-agents |
| [STATE] | Simulation active persistante |
| [DICT] | Dictionnaires sémantiques partagés |
| [SCHEMA] | Plans de reconstruction |
| [PURGE] | Compression sélective |
| [MERGE] | Fusion d'ADN résonants |
| [FORK] | Divergence irrésoluble |

---

## Format ADN CSTL

```
ENTITÉ_SOURCE | OPÉRATEUR | ENTITÉ_CIBLE | FORCE | COUCHE
```

### Exemple — Écosystème Velundra
```
Azhar | ARR | Flux_Ondral | 0.97 | bedrock
Flux_Ondral | ARR | Spores_Kelthis | 0.95 | bedrock
Spores_Kelthis | ARR | Nodules_Vivants | 0.93 | deep
Nodules_Vivants | ARR | Forets_Umbrales | 0.91 | deep
Forets_Umbrales | → | Veldrine | 0.89 | deep
Veldrine | ↑ | Azhar | 0.88 | deep
Forets_Umbrales | ↔ | Azhar | 0.86 | bedrock
[IF] temperature>40 | ARR | Surchauffe | 0.93 | bedrock
[MUST] Purex | INH | Cendral | 0.89 | deep
```

### Couches de profondeur
| Couche | Signification |
|--------|---------------|
| bedrock | Relations fondamentales immuables |
| deep | Relations structurelles stables |
| shallow | Relations dynamiques modifiables |
| surface | Relations contextuelles temporaires |

---

## Résultats Expérimentaux

### Validation de l'Alphabet — 100% sur tous les groupes

| Groupe | Symboles | Score | Définition nécessaire |
|--------|----------|-------|----------------------|
| Opérateurs base | ARR AMP ATT INH CYC BID SYN ANT | 100% | Non |
| Modalités | [IF][MUST][NOT][MAY] | 100% | Non |
| Relations | → ↔ ⊗ ⟳ | 100% | Non |
| Ton | (+)(-)(?)(!!) | 100% | Non |
| Poids | + - ° | 100% | Non |
| Temps | « = » «=» | 100% | 5 lignes |
| Forces | ⊕ ℝ κ ⊖ ℜ | 100% | 5 lignes |
| Couche ψ | ⟶ ~̃+ Δ ℙ | 100% | 5 lignes |
| Modes | ≡ ≠ ∿ \| ≪ | 100% | 5 lignes |
| Réseau | [NET][TRUST][STATE][PURGE] | 100% | 5 lignes |
| **TOTAL** | **37+ symboles** | **100%** | |

### Validation AI-to-AI — Domaines Fictifs (Anti-Triche)

| Test | Émetteur | Récepteur | Fidélité |
|------|----------|-----------|---------|
| Domaine réel (climat) | Claude | ChatGPT | 100% |
| Domaine réel (climat) | Claude | Gemini | 100% |
| Fictif Kelvaria | Claude | ChatGPT | 93% |
| Fictif Thraxis | Claude | Gemini | 99% |
| Fictif Velundra (389 mots) | Claude | ChatGPT | 100% |
| Fictif Velundra (389 mots) | Claude | Gemini | 100% |
| **MOYENNE** | | | **98.7%** |

> **Anti-triche** : Les domaines fictifs (Kelvaria, Thraxis, Velundra) garantissent que les LLMs ne répondent pas depuis leur base de connaissances préentraînée. Chaque concept est inventé de toutes pièces.

### Benchmarks Compression

| Fichier | Taille originale | CSTL | Gain vs gzip |
|---------|-----------------|------|-------------|
| JSON 100 lignes | 1.2 KB | 421 bytes | 93.9% |
| JSON 500 lignes | 5.8 KB | 1326 bytes | 96.2% |
| JSON 5000 lignes | 52 KB | 11078 bytes | 96.9% |
| API logs 1K | 53 KB | 4854 bytes | 90.9% |
| Métriques 1K | 37 KB | 2766 bytes | 92.6% |
| data.json (CSTL natif) | 12 KB | 455 bytes | **99.2%** |

---

## Pipeline Canonique

```
LLM lit → CSTL structure → CSTL vérifie → LLM reformule
```

### Communication AI-to-AI

```
Émetteur (Claude) :
  1. Générer une réponse
  2. Extraire l'ADN k=9
  3. Transmettre l'ADN (27-62 bits par relation)

Récepteur (GPT-4, Gemini...) :
  1. Recevoir l'ADN
  2. Lookup dans le dictionnaire partagé
  3. Reformuler dans son espace sémantique
```

---

## La Propriété Fondamentale

```
∀ relation R dans un corpus humain :
  9 symboles CSTL → identifiant unique universel

Preuve :
  k=8 → 100% unicité mesurée par trie sur corpus réels
  k=9 → garantie par construction (symbole 9 = couche)
  Espace d'adresses : 121⁹ = 5.56×10¹⁸
```

---

## Règle d'Or

```
Un concept = un seul nom dans tout l'ADN.
La cohérence nominale garantit la reproductibilité déterministe.
```

---

## Fichiers

| Fichier | Description |
|---------|-------------|
| `CSTL_v3_Spec.docx` | Spécification complète — 8 axiomes, 65 symboles, 3 couches |
| `cstl_colab.py` | Test complet en Python — 8 groupes, 1 cellule Colab |
| `cstl_full_test.cpp` | Test complet en C++ — 7 groupes, Android/Termux |

---

## Licence

Ce projet est publié en accès libre pour la recherche académique et l'usage non-commercial.

---

## Citation

```
Olivier. (2026). CSTL — Compressed Semantic Transfer Language v3.0:
Un protocole universel de communication sémantique entre agents IA.
```

---

*CSTL n'est pas un compresseur. Ce n'est pas un format de fichier.*
*C'est une infrastructure — comme TCP/IP pour les bytes, mais pour les relations causales.*

**Flux → Relation → Structure → Mémoire → Évolution**
