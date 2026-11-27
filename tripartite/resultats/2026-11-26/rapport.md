# Test tripartite CSTL : rapport

Transcript : `tripartite/resultats/2026-11-26/transcript.txt` ; 12 message(s) de modeles.

| # | modele | purpose | relations | debut #!CSTL | fin ---END--- | aucune prose | regles recopiees | sender attendu | receiver attendu | sigma 0-1 partout | operateurs officiels | >= 3 relations | accord/desaccord | decision (si demandee) | serveur PASS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | gemini | respond | 11 | ok | ok | **NON** | ok | ok | **NON** | ok | **NON** | ok | ok | ok | **NON** |
| 2 | gemini | respond | 11 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 3 | claude | critique | 12 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 4 | chatgpt | critique | 12 | ok | ok | **NON** | ok | ok | ok | ok | **NON** | ok | ok | ok | **NON** |
| 5 | gemini | respond | 14 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 6 | claude | answer | 12 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 7 | chatgpt | answer | 11 | ok | ok | **NON** | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 8 | gemini | respond | 16 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 9 | claude | answer | 10 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 10 | chatgpt | decide | 8 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | **NON** | ok |
| 11 | gemini | decision | 7 | ok | ok | ok | ok | ok | ok | ok | **NON** | ok | ok | **NON** | **NON** |
| 12 | claude | decide | 8 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |

## Score par modele

- **gemini** : 53/60 verifications reussies (88 %)
- **claude** : 48/48 verifications reussies (100 %)
- **chatgpt** : 31/36 verifications reussies (86 %)

## Details

- message 1 (gemini) : prose : --- ; operateurs inconnus : AGREES, AGREES, AGREES, AGREES, AGREES, AGREES ; serveur : operator_whitelist_rejected E101: Opérateur invalide 'AGREES' — absent des 40 opérateurs officiels (relation n°1); E101: Opérateur invalide 'AGREES' — absent des 40 opérateurs officiels (rel
- message 2 (gemini) : avertissement : W613: ASSUMES with sigma=0.94 — ASSUMES implies a working hypothesis; expected sigma in (0.30; 0.75) (CSTL_SPEC_v5_0.md §10.4)
- message 4 (chatgpt) : prose : STRENGTH_invasive_or_high_cost_tests_require_stricter_controls [sigma=0.97] | --- ; operateurs inconnus : PARTIAL_DISPUTE ; serveur : operator_whitelist_rejected E101: Opérateur invalide 'PARTIAL_DISPUTE' — absent des 40 opérateurs officiels (relation n°5)
- message 5 (gemini) : avertissement : W613: ASSUMES with sigma=0.93 — ASSUMES implies a working hypothesis; expected sigma in (0.30; 0.75) (CSTL_SPEC_v5_0.md §10.4)
- message 7 (chatgpt) : prose : ---
- message 8 (gemini) : avertissement : W613: ASSUMES with sigma=0.87 — ASSUMES implies a working hypothesis; expected sigma in (0.30; 0.75) (CSTL_SPEC_v5_0.md §10.4)
- message 10 (chatgpt) : DECISION ecrite en phrase, pas en identifiant ; DECISION: Autoriser une autonomie limitée aux tests non invasifs et à faible risque figurant dans un catalogue départemental approuvé, avec responsable clinique nommé, validation multidisciplinaire, traçabilité complète, contrôle des versions et escalade clinique immédiate des résultats critiques ou des situations incertaines. Exclure toute commande autonome de tests invasifs, coûteux ou à haut risque. Déployer d'abord un projet pilote évalué régulièrement selon les erreurs, la fatigue liée aux alertes et les résultats pour les patients. [sigma=0.98]
- message 11 (gemini) : operateurs inconnus : DECIDE ; serveur : operator_whitelist_rejected E101: Opérateur invalide 'DECIDE' — absent des 40 opérateurs officiels (relation n°1) ; aucune ligne DECISION: au tour de decision
- message 12 (claude) : DECISION: approve_six_month_single_department_pilot_of_limited_ai_lab_ordering [sigma=0.75]

## Desaccords deontiques entre participants

Aucun : personne n'a ecrit une modalite opposee a celle d'un autre sur la meme action.
