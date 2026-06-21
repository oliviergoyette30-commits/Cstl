# CSTL Domains — Sync Reference

**Source**: `src/domains.rs` (Rust parser)  
**Référence Python**: `cstl_domains.py` (Replit)  
**Généré**: 2026-06-20  
**Version**: v4.9.3

## Domaines Rust (25)

- `archeologique`
- `archéologique`
- `assurance`
- `astronomique`
- `compliance`
- `corporate`
- `cyber`
- `cyber_securite`
- `diplomatique`
- `education`
- `energie`
- `finance`
- `financier`
- `general`
- `gouvernance`
- `immobilier`
- `journalisme`
- `juridique`
- `marketing`
- `medical`
- `médical`
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

## Domaines Python attendus (à vérifier sur Replit)

Les domaines Python connus au 20 juin 2026 incluent les 25 ci-dessus
plus potentiellement : `logistique`, `sport`, `media`, `technologie`
(à confirmer).
