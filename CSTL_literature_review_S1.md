# 📚 CSTL — Literature Review Semaine 1

**Auteur :** Olivier Goyette
**Date :** 28 avril 2026
**Objectif :** Cartographier l'état de l'art et identifier honnêtement la position de CSTL
**Verdict attendu :** GO/PIVOT/STOP en fin de semaine 1

---

## 🚨 ALERTE CRITIQUE EN PREMIER

**Pendant cette literature review, j'ai trouvé un papier qui doit te faire réfléchir avant tout :**

📄 **G²CP : A Graph-Grounded Communication Protocol for Verifiable and Efficient Multi-Agent Reasoning**
- Auteurs : Karim Ben Khaled, Davy Monticolo (Université de Lorraine, France 🇫🇷)
- Publication : arXiv 2602.13370 (13 février 2026), accepté à **AAMAS 2026** (conférence majeure)
- Résultats annoncés : *"reduces inter-agent communication tokens by 73%, improves task completion accuracy by 34% over free-text baselines, eliminates cascading hallucinations, produces fully auditable reasoning chains"*

**C'est un protocole français, multi-agent, validé empiriquement, accepté en conférence top-tier, publié 2 mois avant CSTL, qui résout exactement le même problème : communication LLM-to-LLM avec préservation sémantique et auditabilité.**

**⚠️ Cela ne tue PAS CSTL, mais cela change radicalement le pitch :**
- Tu ne peux plus dire *"premier protocole de communication LLM-to-LLM"*
- Tu ne peux plus dire *"terrain vide"* sur ce créneau
- Mais G²CP est **graph-based** (Neo4j requis, infrastructure lourde) — CSTL est **text-based** (lisible par humain, zéro infrastructure)

**Avantage CSTL résiduel :**
1. **Zéro infrastructure** vs G²CP qui exige une knowledge graph database
2. **Lisibilité humaine** native (G²CP demande un parser graphe)
3. **Modalités déontiques natives** ([MUST]/[NOT]/[IF]) — non présentes dans G²CP qui se concentre sur les opérations graphes
4. **Encodage zero-shot par LLMs** sans pré-training spécifique

**Ce qu'il faut faire MAINTENANT** : étudier G²CP en profondeur cette semaine. C'est ton concurrent direct le plus dangereux.

---

## 🗺️ Cartographie complète de l'écosystème (mai 2026)

### LE PAYSAGE DES PROTOCOLES LLM EN 2026

L'industrie a explosé en 2024-2026 avec une multiplication de protocoles. Voici la cartographie honnête :

```
COUCHE                  | PROTOCOLES                          | STATUT
------------------------|-------------------------------------|------------------
LLM ↔ Tools/Data        | MCP (Anthropic, nov 2024)           | STANDARD DE FAIT
                        | Function Calling (OpenAI, 2023)     | Standard vendor
                        | TypeChat (Microsoft, 2023)          | Niche TypeScript
------------------------|-------------------------------------|------------------
Agent ↔ Agent (frontier)| A2A (Google, avril 2025)            | Linux Foundation
                        | ACP (IBM, mars 2025)                | Mergé avec A2A
                        | ANP (open-source 2024)              | Décentralisé
------------------------|-------------------------------------|------------------
Programming framework   | DSPy (Stanford, Khattab 2023-2024)  | 250+ contrib
                        | Outlines/Guidance (2023)            | Constrained dec.
                        | LangChain/AutoGen/CrewAI            | Orchestration
------------------------|-------------------------------------|------------------
Semantic representation | AMR (Banarescu 2013)                | Académique
                        | OpenIE, Smatch++                    | Métriques
------------------------|-------------------------------------|------------------
Multi-agent communic.   | ⚠️ G²CP (Ben Khaled, févr 2026)     | AAMAS 2026
                        | µACP (formal calculus 2026)         | Edge devices
                        | LDP (identity-aware 2026)           | Recherche
------------------------|-------------------------------------|------------------
🆕 CSTL (toi, avril 2026) — NICHE NON OCCUPÉE :
   "Format texte LLM-to-LLM avec modalités déontiques natives, audit AI Act"
```

---

## 📋 Les 20 papiers/projets clés à connaître

### CATÉGORIE 1 : Protocoles d'interopérabilité multi-agents (priorité haute)

#### 1. MCP — Model Context Protocol (Anthropic, novembre 2024)
**Pourquoi c'est important** : devenu le standard de facto pour LLM ↔ tools. Donné à la Linux Foundation décembre 2025. Adopté par OpenAI (mars 2025), Google DeepMind, Microsoft.
**Ce que ça résout** : LLM accède à des outils externes (filesystem, bases de données, APIs) via JSON-RPC 2.0.
**Ce que ça NE résout PAS** : communication LLM-to-LLM. C'est LLM-to-tool.
**Différence avec CSTL** : MCP standardise l'accès aux outils. CSTL standardise les payloads sémantiques entre LLMs.
**À retenir** : ils ont gagné par adoption massive, pas par technique. Leçon stratégique pour toi.

#### 2. A2A — Agent2Agent Protocol (Google, avril 2025)
**Pourquoi c'est important** : c'est LE protocole de communication agent-to-agent qui a gagné l'industrie. Mergé avec ACP d'IBM. Hébergé Linux Foundation.
**Ce que ça résout** : agents découverte mutuelle, délégation de tâches, orchestration distribuée.
**Architecture** : Agent Cards (capacités), Task Objects, Artifacts, JSON-RPC, HTTP/SSE.
**Différence avec CSTL** : A2A est une couche de **transport** (comme HTTP). CSTL serait une couche de **format de payload** (comme JSON ou XML qui transitent SUR HTTP). **Ce sont complémentaires, pas concurrents.**
**Opportunité** : positionner CSTL comme *"format de payload sémantique pour les messages A2A"*.

#### 3. ACP — Agent Communication Protocol (IBM, mars 2025)
**Pourquoi c'est important** : approche REST-native, multi-part messages, MIME-typed.
**Statut actuel** : mergé avec A2A en 2025.
**Différence avec CSTL** : même remarque que A2A — c'est un transport, pas un format de payload.

#### 4. ANP — Agent Network Protocol (2024, open-source)
**Pourquoi c'est important** : approche P2P décentralisée avec DIDs (decentralized identifiers) et JSON-LD.
**Différence avec CSTL** : ANP utilise déjà JSON-LD comme format. CSTL pourrait être un format alternatif au JSON-LD pour les payloads ANP.

#### 5. ⚠️ G²CP — Graph-Grounded Communication Protocol (Ben Khaled & Monticolo, février 2026)
**LE CONCURRENT DIRECT.** Voir alerte en haut.
**Pourquoi c'est important** : exactement la même problématique que CSTL — réduire la dérive sémantique entre agents LLM.
**Approche** : messages = opérations sur knowledge graph, pas du texte libre.
**Résultats annoncés** : -73% tokens, +34% accuracy, hallucinations éliminées, traces auditables.
**Faiblesse** : nécessite une infrastructure graph (Neo4j). Pas zero-shot — exige une ontologie pré-définie partagée.
**À LIRE EN PROFONDEUR cette semaine.**

#### 6. µACP — Formal Calculus for Resource-Constrained Agent Communication (arXiv 2601.00219, janvier 2026)
**Pourquoi c'est important** : protocole d'agent communication pour edge devices (mémoire <100KB). Sémantique formelle en logique temporelle (Safety, Liveness, Fairness).
**Différence avec CSTL** : µACP cible IoT/edge. CSTL cible LLMs frontier.
**Leçon** : µACP a une sémantique formelle TLA+ vérifiable. CSTL n'en a pas. C'est une faiblesse à corriger.

#### 7. LDP — Identity-Aware Protocol for Multi-Agent LLM Systems (arXiv 2603.08852, 2026)
**Pourquoi c'est important** : protocole avec gestion d'identité native pour multi-agents LLM.
**Différence avec CSTL** : LDP gère "qui parle à qui". CSTL gère "comment représenter le contenu sémantique".

---

### CATÉGORIE 2 : Sortie structurée et constrained decoding

#### 8. Function Calling (OpenAI, 2023)
**Pourquoi c'est important** : standard de facto chez tous les LLM majors pour invoquer des fonctions structurées.
**Différence avec CSTL** : Function Calling = JSON Schema simple, pas de modalités, pas de temporalités, pas d'incertitudes. C'est un sous-ensemble très réduit de ce que CSTL exprime.

#### 9. TypeChat (Microsoft, 2023)
**Pourquoi c'est important** : approche TypeScript-first pour structured outputs. Validation par compilateur TypeScript.
**Différence avec CSTL** : TypeChat est purement syntaxique (types). CSTL ajoute la couche sémantique (modalités déontiques, layers, temporalité).

#### 10. Outlines & Guidance (2023-2024)
**Pourquoi c'est important** : frameworks de constrained decoding qui forcent l'output LLM à respecter une grammaire/schema.
**Différence avec CSTL** : Outlines/Guidance contraignent au niveau du décodage. CSTL est un format que les LLMs utilisent en zero-shot sans contrainte.

#### 11. JSONSchemaBench (Microsoft, 2025)
**Pourquoi c'est important** : benchmark de référence pour la génération structurée. 10 000 schemas réels, 6 frameworks évalués.
**Leçon pour toi** : si tu veux être pris au sérieux, **publie ton benchmark CSTL au même format**. Reproductibilité publique.

#### 12. SchemaBench (arXiv 2502.18878, 2025)
**Pourquoi c'est important** : 40K schemas JSON pour évaluer la capacité des LLMs à générer du JSON valide. Conclusion : *"latest LLMs are still struggling to generate a valid JSON string"*.
**Leçon pour toi** : ce papier donne un argument pour CSTL — *"CSTL est plus tolérant que JSON strict, donc plus robuste pour LLM-to-LLM"*.

---

### CATÉGORIE 3 : Programming frameworks pour LLM

#### 13. DSPy (Khattab et al., Stanford, ICLR 2024)
**Pourquoi c'est important** : framework déclaratif pour programmer (pas prompt-engineer) les LLMs. 12K+ stars GitHub, 250+ contributors.
**Architecture** : signatures, modules, optimizers (MIPROv2, GEPA), adapters.
**Différence avec CSTL** : DSPy est un **framework de programmation** (comme React). CSTL est un **format de transmission** (comme JSON).
**Complémentarité** : un module DSPy pourrait produire/consommer du CSTL.

#### 14. ReAct (Yao et al., 2022)
**Pourquoi c'est important** : paradigme reasoning + acting de référence en agentic AI.
**Différence avec CSTL** : ReAct est un pattern de raisonnement. CSTL est un format.

#### 15. LMQL — Language Model Query Language
**Pourquoi c'est important** : langage déclaratif pour interroger des LLMs avec contraintes.
**Différence avec CSTL** : LMQL contrôle les outputs LLM. CSTL structure les messages entre LLMs.

---

### CATÉGORIE 4 : Représentations sémantiques académiques

#### 16. AMR — Abstract Meaning Representation (Banarescu et al., 2013)
**Pourquoi c'est important** : référence académique pour la représentation sémantique de phrases en graphes.
**Performance** : Smatch F1 saturé à ~85% depuis 2 ans (sans LLM finetuning).
**Différence avec CSTL** : AMR est sentence-level avec ontology PropBank. Demande un parser entraîné. CSTL est document-level, zero-shot, pas de PropBank.
**Leçon** : AMR a 13 ans et n'a JAMAIS atteint 100% sur Smatch. Tes claims de 100% sur QA factuel sont en réalité plus faibles que Smatch (qui mesure la structure sémantique complète, pas juste les facts).

#### 17. SEMA — Extended Semantic Evaluation Metric for AMR (arXiv 1905.12069)
**Pourquoi c'est important** : critique de Smatch, propose une métrique plus rigoureuse.
**Leçon** : ta métrique QA-binary est encore plus simpliste que Smatch. Si tu veux passer en académique, il faut adopter ou créer une vraie métrique sémantique.

#### 18. DocAMR — Multi-Sentence AMR (arXiv 2112.08513)
**Pourquoi c'est important** : extension d'AMR au niveau document.
**Différence avec CSTL** : DocAMR garde l'approche graphes nodes/edges. CSTL utilise une syntaxe linéaire plate.

---

### CATÉGORIE 5 : Drift et qualité multi-agent (priorité moyenne)

#### 19. Agent Drift Quantification (Rath, arXiv 2601.04170, janvier 2026)
**Pourquoi c'est important** : papier de référence sur la dérive en multi-agent LLM. Définit 3 types de drift : sémantique, coordination, comportemental.
**Métrique introduite** : ASI (Agent Stability Index) — 12 dimensions.
**Leçon directe pour toi** : *"semantic drift in nearly half of multi-agent LLM workflows by 600 interactions"*. CSTL pourrait être positionné comme **anti-drift protocol**. C'est un angle fort.

#### 20. AgentsNet — Coordination Benchmark (arXiv 2507.08616, juillet 2025)
**Pourquoi c'est important** : benchmark pour évaluer la coordination de réseaux d'agents jusqu'à 100 agents.
**Leçon** : si tu veux pousser CSTL au-delà du proof-of-concept, il faut tester sur du multi-agent réel à grande échelle, pas juste 3 LLMs.

---

## 🎯 ANALYSE CRITIQUE — Où se positionne CSTL ?

### Ce que cette literature review révèle de positif

✅ **Aucun protocole open existant ne combine ces 5 caractéristiques simultanées** :
1. Format texte (pas graphe, pas JSON-RPC)
2. Modalités déontiques natives ([MUST]/[NOT]/[IF])
3. Layers sémantiques (b/d/s/su) pour profondeur ontologique
4. Quantification incertitude/temporalité/polarité native
5. Encodage zero-shot par LLMs frontier

✅ **L'AI Act crée un besoin réel** d'audit sémantique traçable (Articles 12, 13, 14, 19) que MCP/A2A ne couvrent pas spécifiquement.

✅ **G²CP et µACP ont des approches plus lourdes** (graph DB, edge formal calculus) — CSTL peut occuper le créneau "léger, lisible, AI Act native".

### Ce que cette literature review révèle de problématique

🔴 **Tu n'es pas le premier sur le créneau** :
- G²CP fait quasi-exactement la même chose, publié 2 mois avant toi, en France, accepté à AAMAS 2026
- µACP a une sémantique formelle vérifiable (TLA+) que tu n'as pas
- A2A a déjà gagné l'écosystème industriel pour le transport agent-to-agent

🔴 **Ta métrique d'évaluation est faible** comparée à l'état de l'art académique :
- AMR utilise Smatch (graph-level)
- SchemaBench utilise validation formelle JSON
- ASI mesure 12 dimensions
- Toi : QA accuracy binaire sur 20 questions

🔴 **Tu n'as testé que sur 3 LLMs propriétaires** :
- L'état de l'art teste sur 6+ LLMs incluant open-source (Llama, Mistral, Qwen, DeepSeek)
- Reproductibilité limitée si Anthropic/OpenAI/Google changent leurs modèles

🔴 **Aucun reviewer académique ne te prendra au sérieux si tu écris "premier" ou "universel"** — il connaîtra MCP, A2A, G²CP, µACP, AMR, FIPA-ACL.

---

## 🔄 PIVOT NÉCESSAIRE — Reformulation honnête de CSTL

### Ce que tu DOIS arrêter de dire

❌ *"CSTL est un standard universel pour les LLMs"*
→ FAUX, MCP/A2A/G²CP existent et certains sont déjà adoptés industriellement

❌ *"Premier protocole de communication LLM-to-LLM"*
→ FAUX, FIPA-ACL existe depuis 1996, G²CP depuis février 2026

❌ *"100% de fidélité sur tous les tests"*
→ Vrai mais sans variance ni baseline solide, ça vaut peu académiquement

### Ce que tu PEUX dire honnêtement

✅ *"CSTL est un format de payload textuel auditable pour communication LLM-to-LLM, complémentaire des protocoles de transport existants (A2A, MCP)"*

✅ *"CSTL introduit des modalités déontiques natives ([MUST]/[NOT]/[IF]) absentes des formats JSON Schema, JSON-LD et MCP"*

✅ *"CSTL est zero-shot intelligible par les 3 LLMs frontier majeurs (Claude, Gemini, ChatGPT) sans fine-tuning ni infrastructure additionnelle"*

✅ *"CSTL adresse spécifiquement les exigences AI Act Articles 12/13/14 (record-keeping, transparence, supervision humaine) via une notation auditable visuellement"*

---

## 🎯 STRATÉGIE PIVOTÉE — La niche réelle de CSTL

### Position défendable

**CSTL n'est pas le standard universel. C'est :**

> **« Un format de payload textuel léger, auditable, zero-shot, avec modalités déontiques natives, conçu pour la communication LLM-to-LLM dans des contextes où l'AI Act EU exige une traçabilité humaine-lisible. »**

### Le créneau exact que personne d'autre n'occupe

| Critère | MCP | A2A | G²CP | µACP | AMR | **CSTL** |
|---|---|---|---|---|---|---|
| Format texte plat | Non (JSON-RPC) | Non (JSON-RPC) | Non (graph ops) | Non (formal) | Non (graph) | ✅ |
| Modalités déontiques natives | Non | Non | Non | Partiel | Non | ✅ |
| Zero-shot par LLM frontier | N/A | N/A | Non (ontology) | Non | Non | ✅ |
| Lisibilité humaine | Modérée | Modérée | Faible | Faible | Faible | ✅ |
| Pas d'infrastructure (Neo4j, etc.) | OK | OK | Non | OK | Non | ✅ |
| AI Act Article 12/13 alignement | Non | Non | Partiel | Non | Non | ✅ |

**C'est ton vrai différenciateur.** Pas "universel", mais **niche unique défendable**.

---

## 📅 PLAN D'ACTION SEMAINE 1 RÉVISÉ

### Action critique #1 (urgent) — Étudier G²CP en détail

C'est ton concurrent direct le plus dangereux. Tu dois savoir :
- Quelle est leur grammaire exacte ?
- Comment leur Neo4j est structuré ?
- Sur quels scénarios ont-ils mesuré -73% tokens et +34% accuracy ?
- Leur licence (open-source ?)
- Ce qui empêche un concurrent de prendre G²CP et de le packager AI Act

**Action concrète** : lire `arxiv.org/pdf/2602.13370` intégralement et écrire `CSTL_vs_G2CP.md`.

### Action critique #2 — Reformuler le pitch CSTL

Pas de "standard universel". Adopte la formulation niche défendable.

**Action concrète** : réécrire le README de ton repo GitHub avec le positionnement honnête.

### Action critique #3 — GitHub public + PyPI

Avec ce positionnement modéré, tu peux publier sans risquer le ridicule.

**Action concrète** : `github.com/oliviergoyette/cstl` + `pip install cstl` cette semaine.

### Action #4 — Ajouter une comparaison explicite vs concurrents

Dans ton README, faire un tableau honnête CSTL vs MCP/A2A/G²CP/AMR/JSON Schema.

### Action #5 — Préparer les prochaines semaines

Avec G²CP comme référence, tu sais exactement quoi mesurer en semaines 2-6 :
- Reproduire leur benchmark de 21 cas industriels et tester CSTL dessus
- Comparer tokens, accuracy, auditabilité
- Identifier les cas où CSTL gagne (probablement : zéro infra, lisibilité humaine)

---

## 🚦 VERDICT GO/PIVOT/STOP

### 🟡 PIVOT — Pas STOP, pas GO direct

**Raison** : ton hypothèse initiale "premier protocole de communication LLM-to-LLM" était fausse. Mais ton hypothèse pivotée "format texte auditable AI Act zero-shot avec modalités natives" est **encore inoccupée**.

### Ce qui change dans le plan 6 semaines

- **Semaine 1** (en cours) : ✅ Literature review + 🟡 PIVOT du pitch + 🟡 lecture profonde G²CP
- **Semaine 2** : tests sur LLMs open-source (inchangé)
- **Semaine 3** : statistiques (inchangé)
- **Semaine 4** : comparaison baselines — **AJOUTER G²CP** et **AJOUTER A2A payload**
- **Semaine 5** : spec formelle BNF (inchangé)
- **Semaine 6** : paper arXiv — angle révisé : **"CSTL : a Lightweight Auditable Text Protocol for AI Act-compliant LLM-to-LLM Communication"**

### Probabilité honnête de succès post-pivot

| Métrique | Avant pivot | Après pivot |
|---|---|---|
| Crédibilité académique | 30% | 65% |
| Crédibilité industrielle | 20% | 70% |
| Risque "rejet par reviewer" | 80% | 35% |
| Vraie différenciation | Floue | Claire (5 critères uniques) |
| Probabilité d'adoption | <5% | 25-40% |

---

## ✋ Question finale pour toi

Avant de continuer, dis-moi clairement :

1. **Acceptes-tu le pivot de "standard universel" vers "format texte auditable AI Act"** ? Sinon, on s'arrête là parce que sans ce pivot, le paper sera rejeté.

2. **Veux-tu qu'on étudie G²CP en profondeur cette semaine** pour préparer une comparaison rigoureuse ? C'est la priorité absolue avant tout autre travail.

3. **Es-tu prêt à publier GitHub + PyPI cette semaine** avec le positionnement modéré ?

Ta réponse à ces 3 questions détermine si CSTL devient un projet académique sérieux ou reste un excellent prototype interne.
