/// examples/relations_block_syntax_gap_smoke_test.rs -- HISTORIQUE + FIX.
///
/// Ce fichier documentait a l'origine (2026-09-23) une divergence
/// spec/implementation : la spec (CSTL_SPEC_v5_0.md §7/§8/§10) enseigne la
/// forme BLOC pour RELATIONS/CONSTRAINTS/UNCERTAINTY, mais le parser reel ne
/// reconnaissait QUE la forme plate singuliere `RELATION [type=,subject=,
/// object=]`. Un payload ecrit dans la forme bloc produisait
/// `relations: []` SILENCIEUSEMENT -- aucune erreur, aucun avertissement.
/// Recherche exhaustive a l'epoque : zero test dans le depot n'exercait
/// aucun des 3 blocs. Decision du 2026-09-23 (arbitree par l'utilisateur) :
/// documenter la lacune plutot que de la combler dans l'immediat.
///
/// 2026-09-27 -- COMBLE. Sur demande explicite ("il faut etre sur si on est
/// rendu la... que cstl soit operationnel pour pas revenir en arriere
/// apres"), les 3 blocs sont maintenant implementes dans `server/parser.rs` :
/// - `RELATIONS [(sujet) OPERATEUR objet [attrs]]` desucre vers la MEME
///   RELATION plate que la forme singuliere -- E101/E107/etc. la traitent
///   identiquement, zero duplication semantique.
/// - `CONSTRAINTS [(MODALITE) sujet OPERATEUR objet [attrs]]` (ou forme
///   crochet `[MODALITE] ...`) desucre vers une RELATION plate avec
///   `modality=<MODALITE>` -- exactement l'attribut que `semantic.rs`
///   (FORBIDDEN_MODALITIES/REQUIRED_MODALITIES, Axiome D/E107) lit deja.
/// - `UNCERTAINTY [identifiant STATUT [sigma=...]]` -- nouveau champ
///   `payload.uncertainty` (pas de forme plate equivalente prealable).
///
/// Portee assumee : forme MULTI-LIGNE uniquement (ligne d'ouverture exacte
/// "RELATIONS ["/"CONSTRAINTS ["/"UNCERTAINTY [", lignes internes, ligne de
/// fermeture exacte "]"). La forme compacte sur une seule ligne n'est geree
/// par aucun exemple existant dans ce depot -- non implementee ici,
/// deliberement, pas oubliee.
///
/// Ce fichier sert maintenant de garde-fou vivant DANS L'AUTRE SENS : si les
/// 3 blocs cessent un jour de parser correctement (regression), ce test
/// echoue.
use cstl_parser::server::parser::parse_payload;

fn main() {
    let mut failures: Vec<String> = Vec::new();

    // --- RELATIONS [...] : desucre vers `relations`, identique a la forme plate ---
    let relations_payload = r#"#!CSTL v5.0.0 MODE=A
META [encoder=SmokeTest, produced_by=SmokeTest]
INTENT_PAYLOAD [purpose=relations_block_fix, sender=a, receiver=b]
DEFINE Tenant AS Party [id=e001]
DEFINE Landlord AS Party [id=e002]

RELATIONS [
  (Tenant) PERFORM Landlord [id=r001]
]
---END---
"#;
    let parsed = parse_payload(relations_payload).expect("doit parser");
    println!("[RELATIONS] defines={} relations={} warnings={:?}",
        parsed.defines.len(), parsed.relations.len(), parsed.parse_warnings);
    if parsed.defines.len() != 2 {
        failures.push(format!("RELATIONS: defines attendu=2, obtenu={}", parsed.defines.len()));
    }
    if parsed.relations.len() != 1 {
        failures.push(format!("RELATIONS: relations attendu=1 (regression -- le bloc ne desucre plus), obtenu={}", parsed.relations.len()));
    } else {
        let r = &parsed.relations[0];
        if r.get("type").map(String::as_str) != Some("PERFORM")
            || r.get("subject").map(String::as_str) != Some("Tenant")
            || r.get("object").map(String::as_str) != Some("Landlord")
            || r.get("id").map(String::as_str) != Some("r001")
        {
            failures.push(format!("RELATIONS: champs incorrects apres desucrage: {:?}", r));
        }
    }
    if !parsed.parse_warnings.is_empty() {
        failures.push(format!("RELATIONS: avertissements inattendus: {:?}", parsed.parse_warnings));
    }

    // --- CONSTRAINTS [...] : desucre vers `relations` + modality= ---
    let constraints_payload = r#"#!CSTL v5.0.0 MODE=A
META [encoder=SmokeTest, produced_by=SmokeTest]
INTENT_PAYLOAD [purpose=constraints_block_fix, sender=a, receiver=b]
DEFINE Tenant AS Party [id=e001]
DEFINE Rent AS Obligation [id=e002]

CONSTRAINTS [
  (MUST) Tenant PERFORM Rent [id=r002]
  [MUST_NOT] Tenant ARR.ACCESS Rent [id=r003]
]
---END---
"#;
    let parsed_c = parse_payload(constraints_payload).expect("doit parser");
    println!("[CONSTRAINTS] relations={} warnings={:?}", parsed_c.relations.len(), parsed_c.parse_warnings);
    if parsed_c.relations.len() != 2 {
        failures.push(format!("CONSTRAINTS: relations attendu=2, obtenu={}", parsed_c.relations.len()));
    } else {
        let r1 = &parsed_c.relations[0];
        if r1.get("modality").map(String::as_str) != Some("MUST") {
            failures.push(format!("CONSTRAINTS: modality MUST (forme paren) non desucree: {:?}", r1));
        }
        let r2 = &parsed_c.relations[1];
        if r2.get("modality").map(String::as_str) != Some("MUST_NOT") {
            failures.push(format!("CONSTRAINTS: modality MUST_NOT (forme crochet) non desucree: {:?}", r2));
        }
    }

    // --- CONSTRAINTS avec modalite inconnue : la LIGNE est rejetee (warning), pas tout le payload ---
    let bad_modality_payload = r#"#!CSTL v5.0.0 MODE=A
META [encoder=SmokeTest, produced_by=SmokeTest]
INTENT_PAYLOAD [purpose=constraints_bad_modality, sender=a, receiver=b]
DEFINE Tenant AS Party [id=e001]
DEFINE Rent AS Obligation [id=e002]

CONSTRAINTS [
  (NOTAMODALITY) Tenant PERFORM Rent [id=r004]
]
---END---
"#;
    let parsed_bad = parse_payload(bad_modality_payload).expect("doit parser (rejet au niveau ligne, pas payload)");
    if parsed_bad.relations.len() != 0 {
        failures.push(format!("CONSTRAINTS modalite invalide: relations attendu=0, obtenu={}", parsed_bad.relations.len()));
    }
    if parsed_bad.parse_warnings.is_empty() {
        failures.push("CONSTRAINTS modalite invalide: aucun avertissement produit (devrait signaler la ligne rejetee)".to_string());
    }

    // --- UNCERTAINTY [...] : nouveau champ dedie ---
    let uncertainty_payload = r#"#!CSTL v5.0.0 MODE=A
META [encoder=SmokeTest, produced_by=SmokeTest]
INTENT_PAYLOAD [purpose=uncertainty_block_fix, sender=a, receiver=b]
DEFINE Weather AS Concept [id=e001]

UNCERTAINTY [
  e001 ESTIMATED [sigma=0.7]
  r999 UNKNOWN
]
---END---
"#;
    let parsed_u = parse_payload(uncertainty_payload).expect("doit parser");
    println!("[UNCERTAINTY] entries={} warnings={:?}", parsed_u.uncertainty.len(), parsed_u.parse_warnings);
    if parsed_u.uncertainty.len() != 2 {
        failures.push(format!("UNCERTAINTY: entries attendu=2, obtenu={}", parsed_u.uncertainty.len()));
    } else {
        let u1 = &parsed_u.uncertainty[0];
        if u1.get("identifier").map(String::as_str) != Some("e001")
            || u1.get("status").map(String::as_str) != Some("ESTIMATED")
            || u1.get("sigma").map(String::as_str) != Some("0.7")
        {
            failures.push(format!("UNCERTAINTY: premiere entree incorrecte: {:?}", u1));
        }
        let u2 = &parsed_u.uncertainty[1];
        if u2.get("identifier").map(String::as_str) != Some("r999")
            || u2.get("status").map(String::as_str) != Some("UNKNOWN")
        {
            failures.push(format!("UNCERTAINTY: deuxieme entree incorrecte: {:?}", u2));
        }
    }

    println!("\n{}", "=".repeat(70));
    if failures.is_empty() {
        println!("RELATIONS/CONSTRAINTS/UNCERTAINTY (forme bloc, spec §7/§8/§10) sont \
                   maintenant reconnus par le parser reel. Desucrage verifie vers `relations` \
                   (RELATIONS, CONSTRAINTS+modality) et nouveau champ `uncertainty`. Modalite \
                   ou statut invalide rejette la ligne avec avertissement, pas tout le payload.");
    } else {
        println!("ECHEC ({} point(s)) -- regression sur le fix du 2026-09-27:", failures.len());
        for f in &failures {
            println!("  - {}", f);
        }
        std::process::exit(1);
    }
}
