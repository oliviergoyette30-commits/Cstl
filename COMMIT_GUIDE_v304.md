# Guide d'upload GitHub — CSTL v3.0.4

## Objectif

Déployer v3.0.4 sur `github.com/oliviergoyette30-commits/Cstl` et uploader les
5 CSVs du benchmark dans `experiments/20260419/`.

---

## Étape 1 — Remplacer les fichiers racine du repo

Dans la racine du repo, **remplacer** les 3 fichiers suivants (ils existent
déjà en version v3.0.3) :

1. `CSTL_v3_Spec.docx` (nouvelle version v3.0.4, 65.7 KB)
2. `CHANGELOG.md` (section v3.0.4 ajoutée en tête)
3. `README.md` (chiffres v3.0.4 mis à jour)

### Sur mobile (GitHub app ou navigateur)

Pour chaque fichier :
1. Ouvre le fichier existant dans le repo
2. Clique sur le crayon (edit) ou les 3 points → "Replace file"
3. Uploade la nouvelle version depuis `/Téléchargements/`
4. **Commit message** : `v3.0.4 — narrow-scope Rule 12`

---

## Étape 2 — Ajouter les nouveaux fichiers markdown

**Uploader** ces 2 nouveaux fichiers à la racine du repo :

4. `v3.0.4_prompt_system.md` (9.2 KB, nouveau)
5. `v3.0.3_patch_notes.md` (11.7 KB, si pas encore uploadé)

### Via le bouton "Add file" → "Upload files"

- Glisser les fichiers
- **Commit message** : `Add v3.0.4 prompt system and v3.0.3 patch notes`

---

## Étape 3 — Uploader les 5 CSVs dans experiments/20260419/

Naviguer dans le repo vers `experiments/20260419/` (créer le dossier s'il
n'existe pas).

**Uploader les 5 CSVs** (dans cet ordre de priorité) :

1. **`e3_plus_plus_comparative_v303_v304.csv`** (113 KB) — **benchmark FINAL v3.0.4**
2. `e3_plus_plus_comparative_v302_v303.csv` (114 KB) — intermédiaire
3. `e3_plus_plus_v2_strict_results.csv` (101 KB) — baseline JSON-strict
4. `e3_plus_plus_results.csv` (101 KB) — JSON verbose (biais méthodologique)
5. `e3_v302_benchmark.csv` (si pas déjà uploadé) — 30 phrases

### Commit message

```
Add E3++ benchmark results (5 CSVs, trajectory v3.0.2 → v3.0.4)

- e3_plus_plus_comparative_v303_v304.csv: final v3.0.4 validation
  (0.975 EN / 0.940 FR, +0.120/+0.160 vs JSON-strict, 91% invariance)
- e3_plus_plus_comparative_v302_v303.csv: intermediate v3.0.3 test
  (documents 10 regressions leading to v3.0.4 fix)
- e3_plus_plus_v2_strict_results.csv: v3.0.2 baseline with strict JSON
- e3_plus_plus_results.csv: initial v1 run with verbose JSON
  (documents methodological artifact — JSON verbatim copying)
- e3_v302_benchmark.csv: 30-sentence targeted benchmark
```

---

## Étape 4 — README dans experiments/20260419/

Si le fichier `experiments/20260419/README.md` n'existe pas, le créer avec ce
contenu minimal (ou l'éditer s'il existe déjà pour ajouter la ligne v3.0.4) :

```markdown
# E3++ Experimental Results — 2026-04-19

Full empirical trajectory for CSTL v3.0.2 → v3.0.4.

## Files

- `e3_v302_benchmark.csv` — 30 targeted sentences (E3+ validation of v3.0.2 audit).
- `e3_plus_plus_results.csv` — E3++ v1, 100 EN/FR pairs, verbose JSON baseline.
  ⚠️ Methodological artifact: verbose JSON embedded source text verbatim.
  See v2 for corrected baseline.
- `e3_plus_plus_v2_strict_results.csv` — E3++ v2, 100 pairs, strict JSON enum.
  CSTL v3.0.2: 0.980 EN / 0.840 FR. JSON-strict: 0.835 EN / 0.765 FR.
- `e3_plus_plus_comparative_v302_v303.csv` — v3.0.3 test.
  Revealed over-generalization of Rule 12 (10 regressions).
- `e3_plus_plus_comparative_v303_v304.csv` — **FINAL v3.0.4 validation.**
  CSTL v3.0.4: 0.975 EN / 0.940 FR. JSON-strict: 0.855 EN / 0.780 FR.
  Advantage: +0.120 EN / +0.160 FR. Cross-lingual invariance: 91%.
  Wins: CSTL 35 / JSON 5 / ties 60. Ratio 7:1.

## Methodology

- Encoder: Claude Opus 4.5 (temperature default)
- Judge: Claude Opus 4.5 (same session)
- Baseline: JSON-strict (enum-only values, no free text)
- Evaluation: "preserved / partial / lost" trinary verdict
- Scoring: preserved=1.0, partial=0.5, lost=0.0
```

---

## Vérification finale

Après upload, vérifier dans GitHub que :

- [ ] `CSTL_v3_Spec.docx` mentionne bien "Spécification Unifiée v3.0.4" (ouvrir le fichier)
- [ ] `CHANGELOG.md` commence par `## [3.0.4]` en haut
- [ ] `README.md` affiche le badge `spec-v3.0.4`
- [ ] Les 5 CSVs sont dans `experiments/20260419/`
- [ ] `v3.0.4_prompt_system.md` est à la racine

---

## Après l'upload

**Révoquer toutes les clés API** utilisées dans la journée sur
`console.anthropic.com`.

**Le repo est prêt pour la rédaction du préprint arXiv.**
