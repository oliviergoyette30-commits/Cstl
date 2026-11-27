# CSTL Domains — Sync Reference

**Source**: `src/domains.rs` (Rust parser)
**Référence Python**: `cstl_domains.py` (Replit) — non vérifiable depuis ce
sandbox (pas d'accès réseau vers Replit ici) ; ce document ne reflète que le
côté Rust, à jour.
**Régénéré**: 4 septembre 2026 (audit du repo — l'ancienne version, générée le
2026-06-20, listait 25 entrées : 7 étaient soit des doublons ASCII/accentués
d'un même domaine — `archeologique`/`archéologique`, `medical`/`médical`,
`finance`/`financier`, `cyber`/`cyber_securite` — soit des domaines qui
n'existent nulle part dans `src/domains.rs` — `compliance`, `general`,
`gouvernance` — et n'ont jamais été trouvés ailleurs dans ce dépôt non plus)
**Version**: v5.0.0

## Domaines Rust (18, canonique)

Extrait directement des clés `match` de `get_domain_operators_slice` dans
`src/domains.rs` — la seule source de vérité réelle depuis que
`list_domains()` (qui dupliquait cette liste en dur, jamais réellement
utilisée que par elle-même et son propre test, voir CHANGELOG) a été
retirée le 2026-09-04.

- `archéologique`
- `assurance`
- `astronomique`
- `corporate`
- `cyber_securite`
- `diplomatique`
- `education`
- `energie`
- `financier`
- `immobilier`
- `journalisme`
- `juridique`
- `marketing`
- `médical` (avec fallback ASCII `medical`, géré par `.to_lowercase()` dans le
  code — pas une entrée de liste séparée)
- `recherche`
- `reglementaire`
- `rh`
- `supply_chain`

## Instructions sync

Pour vérifier la cohérence avec cstl_domains.py (sur Replit) :
1. Exporter les clés de DOMAIN_ONTOLOGIES depuis cstl_domains.py
2. Comparer avec cette liste
3. Tout domaine présent dans Python mais absent ici = à ajouter dans domains.rs
4. Tout domaine présent ici mais absent Python = à documenter comme Rust-only

**Non fait depuis au moins le 20 juin 2026** — cette synchronisation n'a pas
été revérifiée contre Replit lors de cet audit (accès réseau indisponible
depuis ce sandbox). La liste ci-dessus est correcte pour le code Rust tel
qu'il tourne aujourd'hui ; elle ne garantit rien sur l'état actuel du côté
Python.
