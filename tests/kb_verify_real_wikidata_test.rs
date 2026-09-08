//! Test d'integration contre le VRAI wikidata.org (endpoints de production,
//! `KbVerifier::new()` -- pas `with_endpoints`, pas de mock).
//!
//! Marque `#[ignore]`: `cargo test` (et `cargo test --release`, la suite
//! normale que ce depot fait passer a 100% sans reseau) NE LANCE JAMAIS ce
//! test. Il ne s'execute que sur demande explicite via
//! `cargo test --release -- --ignored`, precisement pour ne jamais faire
//! dependre la suite normale (CI, sandbox sans acces reseau sortant, etc.)
//! d'un service tiers qui peut etre absent, lent ou throttle.
//!
//! Etat de verification honnete (2026-09-08):
//! - Ce fichier a ete ECRIT et COMPILE (`cargo test --release --no-run`)
//!   dans le sandbox de cette session.
//! - Il n'a PAS ete EXECUTE dans ce sandbox: `curl -sS --max-time 8
//!   "https://www.wikidata.org/w/api.php?action=query&format=json&titles=Paris"`
//!   lance depuis cet environnement echoue immediatement avec
//!   `curl: (56) CONNECT tunnel failed, response 403` -- le proxy sortant de
//!   ce sandbox refuse la connexion vers wikidata.org avant meme d'atteindre
//!   la couche applicative. Aucun resultat reseau reel (succes ou echec) n'a
//!   donc pu etre observe ICI. Personne n'a fabrique de resultat a la place.
//! - A executer sur la machine d'Olivier (acces reseau normal) avec:
//!     cargo test --release --test kb_verify_real_wikidata_test -- --ignored --nocapture
//!   pour obtenir la premiere confirmation reelle contre le vrai service.
//!
//! Choix du cas d'entree: Marie Curie / Pierre Curie, relation "spouse"
//! (P26). Couple choisi plutot qu'une paire plus exotique car (a) les deux
//! entites sont des elements Wikidata tres etablis et peu susceptibles
//! d'etre supprimes ou renommes, et (b) le fait "epoux/epouse" est une
//! propriete biographique stable, non sujette a revision editoriale
//! contrairement par exemple a une population ou une frontiere.

use cstl_parser::kb_verify::KbVerifier;

#[tokio::test]
#[ignore]
async fn test_verify_relation_against_real_wikidata_curie_spouse() {
    let verifier = KbVerifier::new();

    let result = verifier
        .verify_relation("Marie Curie", "spouse", "Pierre Curie", "en", 4, 40)
        .await;

    eprintln!("resultat reel wikidata.org: {result:#?}");

    // Les deux entites doivent au minimum se resoudre en QID reels sur le
    // vrai index Wikidata -- si ceci echoue, wbsearchentities a change de
    // comportement ou le reseau a une panne, a documenter dans le README
    // en meme temps que le resultat de ce test.
    assert!(
        result.subject_qid.is_some() && result.object_qid.is_some(),
        "sujet et objet doivent se resoudre en QID Wikidata reels: {result:#?}"
    );

    // La relation "spouse" (P26) entre Marie et Pierre Curie est un fait
    // biographique bien etabli sur Wikidata depuis des annees -- attendu
    // confirme directement (pas via chaine transitive, P26 n'est pas dans
    // is_chainable).
    assert_eq!(
        result.verified, "confirmed_external_source",
        "relation spouse Marie/Pierre Curie attendue confirmee directement sur le vrai wikidata.org: {result:#?}"
    );
}

/// Controle complementaire, plus simple: une seule entite tres stable doit
/// se resoudre en son QID bien connu (Q7186 = Marie Curie). Sert a isoler
/// un probleme de recherche d'entite (`search_entity`/`wbsearchentities`)
/// d'un probleme specifique a la verification de relation ci-dessus.
#[tokio::test]
#[ignore]
async fn test_search_entity_resolves_marie_curie_on_real_wikidata() {
    let verifier = KbVerifier::new();

    // verify_relation avec un predicat inconnu et un objet identique au
    // sujet est le chemin le plus court pour exercer search_entity() sans
    // dupliquer sa logique privee ici (elle n'est pas pub(crate)).
    let result = verifier
        .verify_relation("Marie Curie", "unmapped_predicate_probe", "Marie Curie", "en", 1, 1)
        .await;

    eprintln!("resultat reel wikidata.org (self-check Marie Curie): {result:#?}");
    assert_eq!(
        result.subject_qid.as_deref(),
        Some("Q7186"),
        "Marie Curie doit se resoudre en Q7186 sur le vrai wikidata.org: {result:#?}"
    );
}
