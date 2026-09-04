//! examples/hypothesis_engine_demo.rs -- demo hors-reseau du moteur d'hypotheses
//! (Level 4, src/hypothesis_engine.rs), livre le 2026-09-04 en reponse a une
//! trouvaille d'audit ("l'etape generative n'a jamais ete demontree").
//!
//! Ce binaire NE fait AUCUN appel reseau -- il construit deux ensembles de
//! voisins Wikidata a la main (les vrais voisins de Marie Curie/Pierre Curie
//! seraient recuperes via `KbVerifier::query_generic_neighbors` sur une vraie
//! machine avec acces reseau a wikidata.org, bloque dans ce sandbox) pour
//! montrer le calcul de bout en bout: recouvrement -> sigma -> formatage CSTL.
//!
//! Pour la vraie verification en direct contre Wikidata (impossible ici,
//! wikidata.org retourne 403 sur le proxy sortant de ce sandbox), lance sur ta
//! machine quelque chose comme:
//!
//!   let verifier = cstl_parser::kb_verify::KbVerifier::new();
//!   let candidates = vec!["Q37463".to_string()]; // Irène Joliot-Curie
//!   let hyps = verifier.detect_entanglement("Q7186", &candidates, 200, 3, "en").await;
//!   for h in hyps { println!("{}", h.to_cstl_relation()); }
//!
//! (Q7186 = Marie Curie sur Wikidata.)

use cstl_parser::hypothesis_engine::EntanglementHypothesis;
use std::collections::HashSet;

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn main() {
    // Voisinage simule de deux entites qui partagent beaucoup de contexte
    // (co-lauréats, meme domaine, meme institution) mais n'ont, dans ce jeu de
    // donnees simule, AUCUNE relation directe deja connue entre elles.
    let marie_curie_neighbors = set(&[
        "Q_Nobel_Physics_1903", "Q_Sorbonne", "Q_Poland", "Q_Radioactivity",
        "Q_Henri_Becquerel", "Q_Pierre_Curie",
    ]);
    let candidate_neighbors = set(&[
        "Q_Nobel_Physics_1903", "Q_Sorbonne", "Q_Radioactivity", "Q_Henri_Becquerel",
    ]);

    let common = marie_curie_neighbors.intersection(&candidate_neighbors).count();
    let overlap = cstl_parser::hypothesis_engine::overlap_coefficient(
        &marie_curie_neighbors, &candidate_neighbors,
    );

    let hypothesis = EntanglementHypothesis::new(
        "Q7186", "Q_candidate_demo",
        Some("Marie Curie".to_string()), Some("Candidat simulé".to_string()),
        common, overlap,
    );

    println!("Voisins communs: {common}");
    println!("Coefficient de recouvrement: {:.2}", hypothesis.overlap_coefficient);
    println!("Sigma genere (delibrement bas, jamais BELIEVES/KNOWS): {:.2}", hypothesis.sigma);
    println!();
    println!("Relation CSTL proposee:");
    println!("{}", hypothesis.to_cstl_relation());

    assert!(hypothesis.sigma < 0.8, "sigma ne doit jamais atteindre le seuil KNOWS");
    assert!(hypothesis.common_neighbors >= 3, "le seuil min_common_neighbors typique doit etre atteint dans cette demo");
    println!();
    println!("✅ Pipeline recouvrement -> sigma -> formatage CSTL verifie hors-reseau.");
    println!("   L'orchestration reseau reelle (detect_entanglement contre Wikidata) reste");
    println!("   a verifier sur une machine avec acces a wikidata.org (bloque ici).");
}
