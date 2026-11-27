# Principes fondateurs — CSTL

**Auteur :** Olivier Goyette
**Version :** 5.0.0

---

## Principe fondateur

> **Les relations sont plus importantes que l'information.**

Ce principe gouverne chaque décision de conception de CSTL. Face à un arbitrage
technique, la question est toujours : *est-ce que ce choix préserve la relation ?*

Il se traduit directement dans le format :

- `based_on=`, `serves_desire=`, `context=` — déclarer une fois, référencer partout,
  ne jamais répéter. CSTL transporte des **pointeurs vers le sens**, pas le sens lui-même.
- Les opérateurs relationnels et déontiques sont des citoyens de première classe,
  pas des métadonnées optionnelles.
- La provenance (`produced_by`, `PARENT_HASH`) relie chaque affirmation à son origine.

**Ascendance intellectuelle.** Ce principe reprend l'intuition structuraliste
(Saussure, 1916) selon laquelle les éléments d'un système n'ont pas de valeur
absolue en eux-mêmes : ils ne prennent sens que par les relations qui les unissent
et les opposent. CSTL applique cette intuition au transport sémantique entre agents.

---

## Corollaire

> **L'information est et restera muette face à un vide relié qui parle.**

Ce corollaire n'est pas une métaphore : c'est la description exacte de la seule
capacité que CSTL a démontrée empiriquement et que ses concurrents n'ont pas.

**Un vide qui parle, en CSTL :**

```
renal_function POSSESSES value [
  sigma=0.3
  UNKNOWN=true
  pending_measurement=next_checkup
]
```

La valeur est absente. Mais l'absence porte son incertitude quantifiée, son statut
épistémique explicite, et son plan de résolution. Ce vide **dit quelque chose**.

**Le même vide, en JSON / MCP / A2A / graphe de connaissance naïf :**

```json
"renal_function": null
```

Vide également — mais **muet**. Impossible de distinguer « on ne sait pas encore »
de « ça n'existe pas » ou de « le champ n'a pas été rempli ».

**Vérification empirique.** Test comparatif à formats natifs (31 août 2026), trois
juges LLM indépendants (GPT, DeepSeek, Gemini). Sur la nuance « distinction
structurelle entre incertitude explicite et simple absence de donnée », le verdict
est **unanime** : le graphe de connaissance naïf ne peut pas l'exprimer. C'est le
seul résultat parfaitement unanime du test.

Autre application du même principe : `PARENT_HASH=ORCHESTRATOR_PENDING` — plutôt
qu'un hash inventé ou un champ vide, un vide honnêtement étiqueté qui déclare
« ce n'est pas à moi de remplir ça ».

---

## Pourquoi les deux principes restent séparés

Le **principe fondateur** est actionnable : il sert de règle de décision pendant
la conception.

Le **corollaire** est démonstratif : il explique *pourquoi* le principe tient, et
se vérifie par la mesure.

Fusionner les deux produirait une formule plus élégante mais moins utile —
on perdrait la règle de travail au profit de la signature.
