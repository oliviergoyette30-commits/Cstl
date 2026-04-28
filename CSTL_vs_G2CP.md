# 🔬 CSTL vs G²CP — Analyse Comparative Approfondie

**Auteur :** Olivier Goyette
**Date :** 28 avril 2026
**Objectif :** Comprendre exactement ce que G²CP fait, où il gagne, où CSTL gagne, et où la concurrence est réelle.
**Méthode :** Analyse du paper arXiv 2602.13370 (Ben Khaled & Monticolo, AAMAS 2026).

---

## 🎯 TL;DR Exécutif

G²CP et CSTL résolvent **le même problème fondamental** (semantic drift en multi-agent LLM) avec **deux philosophies opposées** :

| Dimension | G²CP | CSTL |
|---|---|---|
| **Philosophie** | Structurelle (graphe) | Linguistique (texte) |
| **Pré-requis** | Knowledge graph partagé pré-construit | Aucun |
| **Lisibilité humaine** | Faible (Cypher technique) | Élevée (proche prose) |
| **Modalités déontiques** | Speech acts (FIPA-ACL inspiré) | Modalités natives [MUST]/[NOT]/[IF] |
| **Validation empirique** | 500 scénarios industriels + 21 cas réels | 30+ scénarios synthétiques |
| **Publication** | AAMAS 2026 (top tier) | arXiv en préparation |
| **Code** | GitHub karim0bkh/G2CP_AAMAS | À publier |

**Verdict honnête** : G²CP est **plus mature scientifiquement et industriellement**, mais **pas concurrent direct sur la niche AI Act + lisibilité humaine + zéro infrastructure**. Les deux protocoles peuvent coexister, voire se compléter.

---

## 📖 1. Comprendre G²CP en détail

### 1.1 Le concept central

> *"A G²CP message is like handing a colleague a database query rather than an email — there is no room for misinterpretation."*

**Métaphore G²CP** : agents = bibliothécaires partageant la même base. Au lieu de décrire un livre verbalement (susceptible d'erreur), ils donnent l'identifiant exact (sans ambiguïté).

**Métaphore CSTL** : agents = avocats partageant un format de contrat structuré. Le contrat est lisible directement, mais structuré pour éliminer l'ambiguïté.

### 1.2 Architecture G²CP

Un message G²CP comporte :

1. **Performative** : speech act type (héritage FIPA-ACL)
   - `ASK` : demande d'information
   - `TELL` : transmission d'information
   - `PROPOSE` : proposition d'action
   - `etc.` (basé sur FIPA-ACL ~20 performatives)

2. **Operation** : opération graphe précise
   - `Traversal` : parcours de graphe (MATCH-style Cypher)
   - `Subgraph fragment` : extrait de sous-graphe à partager
   - `Update` : modification de graphe

3. **Knowledge graph cible** : Neo4j ou équivalent

### 1.3 Définition formelle (du paper)

> *Definition 3.1 (Graph Operation). A graph operation `op` has one of three types: traversal, subgraph fragment, or update.*
> *A traversal starts at a set of source nodes and "walks" outward along edges of specified types, collecting everything reachable within a given number of hops.*
> *N(Vs, Ψf) = {vj | ∃vi∈Vs, (vi,vj)∈E, ψ(vi,vj)∈Ψf}*

C'est mathématiquement formel — équivalent à un sous-ensemble de Cypher.

### 1.4 Architecture multi-agents G²CP

Le paper valide G²CP avec 4 agents spécialisés :
- **Diagnostic Agent** : identifie les problèmes
- **Procedural Agent** : propose des procédures
- **Synthesis Agent** : agrège les résultats
- **Ingestion Agent** : maintient le knowledge graph

**Domaine de validation** : maintenance industrielle (industrial knowledge management).

### 1.5 Résultats publiés

| Métrique | Résultat |
|---|---|
| Réduction tokens | **-73%** vs free-text baseline |
| Amélioration accuracy | **+34%** vs free-text baseline |
| Hallucinations | **Éliminées** (cascading) |
| Auditabilité | **Complète** (traces graphes) |
| Tests synthétiques | 500 scénarios |
| Tests réels | 21 cas de maintenance |
| Diagnostic accuracy synth. | 90% |
| Diagnostic accuracy real | 86% |
| Time-to-diagnosis | 47 min → 12 min |

**Impressionnant.** Et beaucoup plus rigoureux que CSTL sur le plan empirique (500 scénarios vs ~30, cas industriels réels vs synthétiques).

---

## 🔍 2. Où G²CP est SUPÉRIEUR à CSTL

Soyons honnêtes. G²CP a 5 avantages que CSTL n'a pas :

### 2.1 Validation empirique massive
- **500 scénarios** synthétiques + **21 cas industriels réels**
- Partenariat avec laboratoire ERPI (Université de Lorraine)
- Industriel a validé les résultats en conditions réelles

CSTL a actuellement ~30 scénarios synthétiques construits par moi-même. **G²CP a 17× plus de validation.**

### 2.2 Publication peer-reviewed top-tier
- Accepté à **AAMAS 2026** (l'IFAAMAS est LA conférence multi-agent systems)
- Reviewers académiques ont validé la rigueur
- Citation officielle : Ben Khaled & Monticolo, 2026

CSTL n'a pas de papier publié. **Différence de crédibilité massive.**

### 2.3 Code public et reproductible
- GitHub : `github.com/karim0bkh/G2CP_AAMAS`
- Datasets, scripts d'évaluation, baselines
- Reproductibilité garantie

CSTL n'a rien de public à ce jour. **Tu pars avec un déficit énorme sur la reproductibilité.**

### 2.4 Élimination déterministe des hallucinations
G²CP **garantit mathématiquement** l'absence d'hallucinations parce que les messages sont des opérations sur un graphe défini. Si l'opération réussit, le résultat est exact. Si elle échoue, c'est explicite.

CSTL **réduit** les hallucinations (validé empiriquement) mais **ne les élimine pas mathématiquement**. Un LLM peut toujours mal interpréter du texte CSTL.

### 2.5 Sémantique formelle vérifiable
G²CP a des théorèmes prouvés :
> *"Theorem 6.1 (Traversal Complexity). A traversal T..."*

CSTL a une grammaire BNF mais **pas de théorèmes formels** sur les propriétés de complétude/correction.

---

## ✅ 3. Où CSTL est SUPÉRIEUR à G²CP

Heureusement, ce n'est pas un combat perdu d'avance. CSTL a 5 avantages distinctifs réels :

### 3.1 Aucune infrastructure requise

**G²CP exige** :
- Un knowledge graph pré-construit (Neo4j, Memgraph, ou équivalent)
- Une pipeline d'entity resolution (string normalization, embedding-based fuzzy matching, domain-expert validation)
- Un schéma graphe défini par avance

**CSTL exige** :
- Rien. Du texte UTF-8.

**Implication concrète** : pour déployer G²CP dans une PME française, il faut d'abord construire le knowledge graph. Coût estimé : 50-200k€ et 3-12 mois. Pour CSTL, une PME peut commencer demain.

**C'est un argument commercial massif.**

### 3.2 Lisibilité humaine native

**Exemple G²CP** (basé sur la sémantique du paper) :
```
PERFORMATIVE: ASK
OPERATION: Traversal
SOURCE_NODES: [n3471, n2891]
EDGE_TYPES: [HAS_FAILURE_MODE, REQUIRES_REPAIR]
MAX_HOPS: 3
RETURN_FORMAT: subgraph
```

Un juriste AI Act qui découvre cela ne comprend rien. Il doit :
1. Connaître Cypher (langage technique)
2. Connaître le schéma graphe (entités, relations)
3. Avoir accès au graphe pour décoder n3471, n2891

**Exemple CSTL équivalent** :
```
[MUST] CreditEval-1.7 ENABLE revision_humaine [sigma=1.0, layer=bedrock]
[NOT] CreditEval-1.7 PERFORM decision_automatique_au_dessus_50000_EUR [sigma=1.0]
```

Un juriste AI Act lit cela directement. Pas besoin de Cypher, pas besoin de schéma, pas besoin de graphe.

**Pour l'AI Act Article 14 (supervision humaine), CSTL gagne sans contestation.**

### 3.3 Modalités déontiques natives en première classe

G²CP utilise des **performatives** (héritage FIPA-ACL : ASK, TELL, PROPOSE) — ce sont des **speech acts**, pas des **modalités déontiques**.

**Différence cruciale** :
- Speech act : *"je te demande X"* (acte de communication)
- Modalité déontique : *"X doit être fait"* (obligation logique)

CSTL distingue clairement :
- `[MUST]` : obligation absolue (déontique)
- `[NOT]` : interdiction absolue (déontique)
- `[IF] X [MUST] Y` : obligation conditionnelle (déontique conditionnelle)

G²CP encode tout via opérations graphes. **Pour exprimer "Le contrat DOIT être livré avant 2026-09-30"**, il faut encoder la deadline comme propriété de nœud, ce qui perd la nuance déontique de l'obligation absolue vs conditionnelle.

**Pour l'AI Act Article 13 (transparence des obligations), CSTL gagne.**

### 3.4 Encodage zero-shot par LLMs frontier

G²CP nécessite que les agents LLM **apprennent le schéma graphe spécifique** au domaine. Le paper mentionne :
> *"Each agent runs a G²CP parser implementing a [domain-specific schema]"*
> *"Node Resolution. Node selectors use a priority system: (1) Explicit IDs via indexed lookup, (2) Type filters via Cypher MATCH..."*

Pour utiliser G²CP dans un nouveau domaine (juridique, médical, financier), il faut :
1. Construire le knowledge graph du domaine
2. Définir les types de nœuds et arêtes
3. Configurer les parsers d'agents

CSTL fonctionne **out-of-the-box** sur tout domaine, validé empiriquement sur 18 domaines (diplomatique, juridique, médical, corporate, etc.). Aucun pré-training spécifique.

**Pour adoption rapide en PME et secteur public, CSTL gagne.**

### 3.5 Niche AI Act non adressée par G²CP

Le paper G²CP **ne mentionne pas** l'AI Act. Pas une seule fois. Leur cas d'usage est la maintenance industrielle. Leurs métriques sont :
- Diagnostic accuracy
- Time-to-diagnosis
- Token reduction

Pas :
- Article 12 compliance (record-keeping)
- Article 13 compliance (transparency)
- Article 14 compliance (human oversight)

**CSTL est positionné AI Act dès la conception.** Bloc CONSTRAINTS, bloc UNCERTAINTY, IDs traçables, modalités natives — tout est aligné sur les exigences réglementaires européennes.

**Pour adoption en Europe sous contrainte AI Act, CSTL gagne.**

---

## 🤝 4. Coexistence possible : CSTL ⊕ G²CP

**Hypothèse non triviale** : ces deux protocoles peuvent **se compléter** plutôt que se concurrencer.

### Pattern d'intégration possible

```
┌─────────────────────────────────────────┐
│ HUMAIN (auditeur AI Act, juriste)       │
│         lit en CSTL                     │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│ COUCHE TRANSCRIPTION                    │
│   CSTL ↔ G²CP (bidirectionnelle)        │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│ AGENTS LLM SPÉCIALISÉS                  │
│   communiquent en G²CP via Neo4j        │
└─────────────────────────────────────────┘
```

**CSTL = couche audit humain.**
**G²CP = couche calcul agent.**

C'est exactement le pattern **JSON ↔ binaire** : JSON pour les humains, Protocol Buffers pour la performance machine. Les deux coexistent parce qu'ils servent des publics différents.

**Cette positionnement coopératif pourrait être ton angle pour le paper arXiv** :
- Pas *"CSTL meilleur que G²CP"* → vexant et difficile à défendre
- Plutôt *"CSTL complémentaire à G²CP : couche audit human-readable"* → defensible et utile

---

## 📊 5. Tableau de comparaison final

| Critère | G²CP | CSTL | Gagnant |
|---|---|---|---|
| **Validation empirique (n)** | 521 | ~30 | G²CP |
| **Publication peer-reviewed** | AAMAS 2026 ✅ | aucune ❌ | G²CP |
| **Code public** | GitHub ✅ | en cours | G²CP |
| **Élimination hallucinations** | mathématique | empirique | G²CP |
| **Sémantique formelle** | théorèmes prouvés | grammaire BNF | G²CP |
| **Infrastructure requise** | Neo4j (lourd) | aucune | **CSTL** |
| **Lisibilité humaine** | faible | élevée | **CSTL** |
| **Modalités déontiques natives** | non (speech acts) | oui [MUST/NOT/IF] | **CSTL** |
| **Encodage zero-shot LLM** | non (schema requis) | oui | **CSTL** |
| **Alignement AI Act** | non mentionné | natif | **CSTL** |
| **Coût de déploiement** | élevé (50-200k€) | nul | **CSTL** |
| **Audience cible** | équipes techniques | équipes mixtes (juristes inclus) | **CSTL** |

**Score : G²CP gagne 5/12, CSTL gagne 7/12.**

Mais c'est trompeur. Les 5 victoires de G²CP sont des **déficits actuels de CSTL** que tu peux combler avec du travail (publication, code, validation). Les 7 victoires de CSTL sont des **différences architecturales** que G²CP ne peut pas combler sans renoncer à son approche graphe.

**Conclusion** : à terme, CSTL peut devenir aussi rigoureux que G²CP. G²CP ne pourra jamais devenir aussi léger et lisible que CSTL.

---

## 🎯 6. Stratégie révisée pour CSTL après cette analyse

### 6.1 Ce que tu dois ABSOLUMENT faire dans les 2 prochaines semaines

1. **Lire le paper G²CP intégralement** (le PDF arxiv.org/pdf/2602.13370 — 13 pages)
2. **Cloner leur code** (github.com/karim0bkh/G2CP_AAMAS) et l'étudier
3. **Comprendre leur méthodologie de benchmark** pour pouvoir t'aligner
4. **Citer G²CP dans ton paper arXiv** dès la première mention de related work

### 6.2 Reformulation du positionnement CSTL

**Avant** (à abandonner) : *"Premier protocole de communication LLM-to-LLM"*

**Après** (defensible) :
> *« CSTL est un format de payload textuel pour communication LLM-to-LLM, optimisé pour l'auditabilité humaine et la conformité AI Act. CSTL adopte une approche linguistique complémentaire à l'approche structurelle de G²CP (Ben Khaled & Monticolo, AAMAS 2026). Là où G²CP excelle dans l'élimination déterministe des hallucinations via opérations graphes (au prix d'une infrastructure lourde et d'une faible lisibilité humaine), CSTL excelle dans le déploiement zero-infrastructure et la lisibilité directe par auditeurs non-techniques. »*

### 6.3 Hypothèses à tester pour différencier réellement CSTL

Pour le paper arXiv, fais ces 3 tests précis qui exploitent les avantages CSTL :

**Test A — Audit humain non-technique**
- Recruter 10 juristes (étudiants en droit suffisent)
- Leur donner un payload CSTL et un payload G²CP équivalent
- Mesurer : combien de modalités/obligations identifient-ils correctement en 5 minutes
- Hypothèse : CSTL bat G²CP par un facteur 3-5×

**Test B — Coût total de déploiement**
- Estimer rigoureusement le TCO (Total Cost of Ownership) sur 1 an pour :
  - G²CP : Neo4j + entity resolution + maintenance KG
  - CSTL : juste un parser Python
- Hypothèse : CSTL coûte 10-100× moins cher

**Test C — Adoption rapide cross-domaine**
- Tester CSTL ET G²CP sur 5 domaines différents (juridique, médical, financier, RH, cyber)
- Mesurer : effort de configuration en heures-homme
- Hypothèse : CSTL plug-and-play, G²CP nécessite ~40h par domaine

Ces 3 tests **différencient empiriquement CSTL de G²CP** sur des dimensions où G²CP n'a pas testé.

---

## 🚦 7. Verdict honnête

### G²CP est-il une menace existentielle pour CSTL ?

**Non.** Pour 3 raisons :

1. **Ils ne ciblent pas le même marché** : G²CP cible la maintenance industrielle avec infrastructure lourde. CSTL cible les PME et secteur public soumis à l'AI Act.

2. **Ils ont des philosophies opposées** : G²CP = structurel, CSTL = linguistique. Les utilisateurs ne choisissent pas entre eux mais selon leur contexte.

3. **G²CP renforce la légitimité du domaine** : leur publication AAMAS 2026 prouve que la communauté académique reconnaît le problème de communication multi-agents LLM. **C'est bon pour CSTL** parce que ça crée un marché.

### G²CP est-il un bénéfice pour CSTL ?

**Oui, paradoxalement.** Pour 4 raisons :

1. **Référence académique solide à citer** : ton paper arXiv aura une citation crédible (Ben Khaled & Monticolo)
2. **Validation que le problème existe** : les reviewers ne peuvent plus dire *"ce problème n'existe pas"* puisqu'AAMAS 2026 l'a accepté
3. **Différenciation claire possible** : tu peux te positionner comme *"alternative légère pour ceux qui ne peuvent pas déployer Neo4j"*
4. **Possibilité d'interopérabilité** : un converter CSTL ↔ G²CP serait un projet valorisable

---

## ✅ 8. Action items concrets

### Cette semaine (priorité 1)
- [ ] Lire intégralement le PDF arxiv.org/pdf/2602.13370 (13 pages, 1-2h)
- [ ] Cloner et explorer github.com/karim0bkh/G2CP_AAMAS (2-3h)
- [ ] Identifier 3-5 différenciateurs CSTL réels et mesurables
- [ ] Réécrire le README CSTL avec le positionnement révisé (couche audit human-readable, complémentaire à G²CP)

### Semaine prochaine (priorité 2)
- [ ] Ajouter G²CP comme baseline dans le test ultime 5 aspects
- [ ] Designer le Test A (audit humain non-technique)
- [ ] Préparer le squelette de paper arXiv avec related work incluant G²CP

### Sur le long terme
- [ ] Considérer un email à Karim Ben Khaled (auteur G²CP) pour discuter d'interopérabilité — collaboration possible plutôt que compétition

---

## 📝 9. Ce que tu DOIS retenir

1. **G²CP existe, est meilleur scientifiquement aujourd'hui, mais ne menace pas la niche CSTL** (AI Act + lisibilité humaine + zéro infrastructure)
2. **L'écart de validation empirique est ton problème principal** — il faut investir 2-4 semaines pour rattraper
3. **Le positionnement "complémentaire" est plus fort** que le positionnement "concurrent"
4. **Cite G²CP dans ton paper** — montrer que tu connais l'état de l'art te crédibilise
5. **Les 7 avantages CSTL sont des différences architecturales réelles** que tu peux défendre rigoureusement

---

## ✋ Question pour toi

Maintenant que tu as cette analyse rigoureuse :

1. **Acceptes-tu le repositionnement "complémentaire à G²CP"** plutôt que "concurrent" ? C'est plus défendable et plus stratégique.

2. **Veux-tu qu'on conçoive le Test A (audit humain non-technique)** dès maintenant ? C'est le test qui va vraiment différencier CSTL empiriquement.

3. **Es-tu prêt à mentionner G²CP de manière professionnelle dans ton paper et ton README** ? C'est obligatoire pour la crédibilité académique.

Réponds et on continue.
