//! src/domain_simulator.rs — Level 4, "Simulation / validation lab".
//!
//! Portee honnete (2026-09-08, en reponse a une trouvaille d'audit: README.md
//! documentait ce composant comme "domain simulators not built"). Ce module
//! ajoute le PREMIER simulateur de domaine reellement branche: un simulateur
//! de PLAUSIBILITE NUMERIQUE/PHYSIQUE. Contrairement a `execution_lab.rs`
//! (cohérence interne, syntaxique: "ce fait contredit-il un autre fait deja
//! connu?"), ce module pose une question differente: "cette valeur est-elle
//! seulement POSSIBLE dans le monde physique reel, independamment de tout
//! autre fait?" — un age humain de 250 ans n'entre en contradiction avec
//! AUCUNE autre relation du payload, mais reste physiquement impossible.
//!
//! Domaine choisi et pourquoi (voir aussi le rapport de session): bornes de
//! plausibilite pour des attributs numeriques typés courants (age, taille,
//! poids, distance, temperature) plus une regle combinee (duree de vie =
//! death_year - birth_year). Ce domaine a ete choisi PARCE QU'il est
//! verifiable unitairement en quelques heures avec des constantes
//! documentees publiquement (records Guinness / mesures meteorologiques
//! officielles), contrairement a un simulateur temporel general (calendaires,
//! fuseaux horaires, granularite jour/heure/minute — bien plus de surface) ou
//! a un simulateur geographique par coordonnees (nécessiterait soit une base
//! de coordonnees embarquee, soit un appel reseau — impossible a verifier
//! dans ce bac a sable, voir kb_verify.rs pour le meme probleme deja
//! rencontre avec wikidata.org).
//!
//! Ce que ce module NE fait PAS: il ne verifie PAS qu'une valeur est
//! *correcte* (ca, c'est le role empirique de kb_verify.rs, avec Wikidata
//! comme source de verite) — seulement qu'elle est *possible* dans le
//! domaine physique. Un age de 45 ans passe le check meme si la vraie
//! reponse est 46: ce module ne connait pas la vraie reponse, seulement les
//! bornes de ce qui peut exister.
//!
//! Verification honnete: toutes les bornes ci-dessous sont des constantes
//! statiques choisies a partir de records publiquement documentes au moment
//! de l'ecriture de ce module (2026-09-08) — PAS interrogees en direct
//! contre une source live (aucun reseau utilise ici, contrairement a
//! kb_verify.rs). Sources indiquees borne par borne. Si un record est battu
//! demain, ces constantes deviennent legerement perimees jusqu'a mise a
//! jour manuelle — ce n'est pas un oracle vivant, c'est un garde-fou
//! physique statique.
//!
//! Branchement live (pas un module orphelin): `execution_lab::
//! check_consistency_with_history` appelle `check_domain_plausibility`
//! ci-dessous et fusionne le resultat dans `ConsistencyReport` — le meme
//! chemin, deja appele par `server/handler.rs` (STEP 3c) pour chaque
//! payload recu, voir `src/execution_lab.rs`.

use std::collections::HashMap;

/// Bornes de plausibilite numerique par predicat: (predicat, min, max, unite,
/// source de la borne). Un objet en dehors de [min, max] pour l'un de ces
/// predicats est physiquement impossible, pas juste "suspect".
///
///   - "age": 0 a 130 ans. Le record humain verifie le plus eleve documente
///     est Jeanne Calment, 122 ans (1997) — 130 est une marge volontairement
///     large au-dessus de tout record verifie connu, pour ne jamais rejeter
///     un cas limite reel par excès de zele.
///   - "height_cm": 40 a 272 cm. Robert Wadlow (272 cm) reste l'humain
///     adulte le plus grand jamais mesure de facon fiable; 40 cm est une
///     borne basse large sous la plus petite taille adulte documentee
///     (Chandra Bahadur Dangi, ~54.6 cm) pour couvrir la petite enfance sans
///     jamais accepter une valeur negative ou nulle.
///   - "weight_kg": 0.3 a 650 kg. 0.3 kg couvre un nouveau-ne extremement
///     petit; 650 kg est au-dessus du poids humain adulte le plus lourd
///     jamais documente (~635 kg, Jon Brower Minnoch).
///   - "distance_km": 0 a 20015 km. 20015 km est la distance antipodale
///     maximale entre deux points a la surface de la Terre (la moitie de la
///     circonference terrestre moyenne, ~40030 km) — aucune distance directe
///     entre deux lieux terrestres ne peut depasser cette valeur.
///   - "temperature_celsius": -89.2 a 56.7. Bornes des records
///     meteorologiques officiels de surface: -89.2°C (station Vostok,
///     Antarctique, 1983) et 56.7°C (Furnace Creek, Death Valley, 1913,
///     record NOAA/OMM reconnu).
pub const NUMERIC_BOUNDS: &[(&str, f64, f64, &str)] = &[
    ("age", 0.0, 130.0, "years"),
    ("height_cm", 40.0, 272.0, "cm"),
    ("weight_kg", 0.3, 650.0, "kg"),
    ("distance_km", 0.0, 20015.0, "km"),
    ("temperature_celsius", -89.2, 56.7, "celsius"),
];

/// Duree de vie humaine maximale plausible, utilisee par
/// `check_lifespan_plausibility` ci-dessous — meme marge que la borne "age"
/// ci-dessus (Jeanne Calment, 122 ans, +marge), pas une nouvelle constante
/// independante qui pourrait diverger.
pub const MAX_HUMAN_LIFESPAN_YEARS: i64 = 130;

/// Predicats consommes par ce module — exactement les cles de
/// `NUMERIC_BOUNDS` plus "birth_year"/"death_year" pour
/// `check_lifespan_plausibility`. Meme role que
/// `execution_lab::relevant_predicates`: permettre a l'appelant de ne
/// charger depuis l'ADN store que ce dont ce module se sert.
pub fn relevant_predicates() -> Vec<&'static str> {
    let mut preds: Vec<&'static str> = NUMERIC_BOUNDS.iter().map(|(p, ..)| *p).collect();
    preds.push("birth_year");
    preds.push("death_year");
    preds
}

/// Une violation de plausibilite: une relation dont l'objet est soit non
/// numerique pour un predicat numerique, soit hors des bornes physiques
/// connues.
#[derive(Debug, Clone, PartialEq)]
pub struct Implausibility {
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlausibilityReport {
    pub plausible: bool,
    pub violations: Vec<Implausibility>,
}

/// Verifie chaque relation dont le predicat est dans `NUMERIC_BOUNDS`:
/// l'objet doit parser en `f64` (accepte le point comme separateur
/// decimal, format `str::parse` standard de Rust) ET tomber dans
/// [min, max]. Un objet non numerique pour un predicat numerique est
/// signale comme implausible avec sa propre raison (distincte d'un
/// depassement de borne) — un `age` valant "beaucoup" n'est pas une valeur
/// hors bornes, c'est une valeur qui n'a pas de sens dans ce champ.
fn check_numeric_bounds(relations: &[HashMap<String, String>]) -> Vec<Implausibility> {
    let mut violations = Vec::new();
    for rel in relations {
        let (Some(subject), Some(predicate), Some(object)) =
            (rel.get("subject"), rel.get("type"), rel.get("object"))
        else { continue };
        let Some(&(_, min, max, unit)) = NUMERIC_BOUNDS.iter().find(|(p, ..)| p == predicate) else { continue };
        match object.parse::<f64>() {
            Ok(value) if value < min || value > max => {
                violations.push(Implausibility {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    value: object.clone(),
                    reason: format!(
                        "{value} {unit} hors des bornes physiques plausibles [{min}, {max}] {unit}"
                    ),
                });
            }
            Ok(_) => {}
            Err(_) => {
                violations.push(Implausibility {
                    subject: subject.clone(),
                    predicate: predicate.clone(),
                    value: object.clone(),
                    reason: format!("valeur non numerique pour un predicat numerique ({predicate})"),
                });
            }
        }
    }
    violations
}

/// Verifie la duree de vie implicite d'un sujet ayant a la fois une relation
/// `birth_year` et `death_year` (dans le payload NOUVEAU combine a
/// l'historique, meme patron de fusion que
/// `execution_lab::check_consistency_with_history`): la mort doit survenir
/// APRES la naissance (duree >= 0) et la duree ne peut pas depasser
/// `MAX_HUMAN_LIFESPAN_YEARS`. Ne rapporte une violation QUE si l'une des
/// deux annees vient du NOUVEAU payload — meme logique de deduplication que
/// le reste de ce fichier et de `execution_lab.rs`: une paire deja
/// entierement dans l'historique aurait deja ete signalee a l'epoque.
fn check_lifespan_plausibility(
    new_relations: &[HashMap<String, String>],
    history_relations: &[HashMap<String, String>],
) -> Vec<Implausibility> {
    let mut birth: HashMap<String, f64> = HashMap::new();
    let mut death: HashMap<String, f64> = HashMap::new();
    let mut new_subjects: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut ingest = |relations: &[HashMap<String, String>], is_new: bool| {
        for rel in relations {
            let (Some(subject), Some(predicate), Some(object)) =
                (rel.get("subject"), rel.get("type"), rel.get("object"))
            else { continue };
            let Ok(year) = object.parse::<f64>() else { continue };
            match predicate.as_str() {
                "birth_year" => { birth.insert(subject.clone(), year); if is_new { new_subjects.insert(subject.clone()); } }
                "death_year" => { death.insert(subject.clone(), year); if is_new { new_subjects.insert(subject.clone()); } }
                _ => {}
            }
        }
    };
    ingest(history_relations, false);
    ingest(new_relations, true);

    let mut violations = Vec::new();
    for (subject, &b) in &birth {
        let Some(&d) = death.get(subject) else { continue };
        if !new_subjects.contains(subject) {
            continue;
        }
        let lifespan = d - b;
        if lifespan < 0.0 {
            violations.push(Implausibility {
                subject: subject.clone(),
                predicate: "death_year".to_string(),
                value: d.to_string(),
                reason: format!("mort ({d}) avant naissance ({b}) — duree de vie negative"),
            });
        } else if lifespan > MAX_HUMAN_LIFESPAN_YEARS as f64 {
            violations.push(Implausibility {
                subject: subject.clone(),
                predicate: "death_year".to_string(),
                value: d.to_string(),
                reason: format!(
                    "duree de vie de {lifespan} ans (naissance {b}, mort {d}) depasse la borne plausible de {MAX_HUMAN_LIFESPAN_YEARS} ans"
                ),
            });
        }
    }
    violations
}

/// Point d'entree unique de ce module, meme patron d'appel que
/// `execution_lab::check_consistency_with_history` (new_relations +
/// history_relations, fusion pour les checks combines). Combine
/// `check_numeric_bounds` (sur le NOUVEAU payload seulement — une borne
/// physique ne depend pas de l'historique, contrairement a un cycle ou une
/// contradiction) et `check_lifespan_plausibility` (qui, lui, a besoin de
/// l'historique pour apparier une moitie passee avec une moitie nouvelle).
pub fn check_domain_plausibility(
    new_relations: &[HashMap<String, String>],
    history_relations: &[HashMap<String, String>],
) -> PlausibilityReport {
    let mut violations = check_numeric_bounds(new_relations);
    violations.extend(check_lifespan_plausibility(new_relations, history_relations));
    PlausibilityReport { plausible: violations.is_empty(), violations }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(subject: &str, predicate: &str, object: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("subject".to_string(), subject.to_string());
        m.insert("type".to_string(), predicate.to_string());
        m.insert("object".to_string(), object.to_string());
        m
    }

    #[test]
    fn test_no_relations_is_plausible() {
        let report = check_domain_plausibility(&[], &[]);
        assert!(report.plausible);
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_plausible_age_passes() {
        let relations = vec![rel("Marie Curie", "age", "66")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_impossible_age_rejected_with_reason() {
        // 250 ans ne contredit aucune autre relation (ce n'est pas le role
        // d'execution_lab.rs) mais est physiquement impossible.
        let relations = vec![rel("Personnage Fictif", "age", "250")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].predicate, "age");
        assert!(report.violations[0].reason.contains("hors des bornes"));
    }

    #[test]
    fn test_negative_age_rejected() {
        let relations = vec![rel("X", "age", "-5")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_boundary_age_values_are_plausible() {
        // Les bornes elles-memes (0 et 130) doivent passer -- inclusives,
        // pas exclusives.
        let relations = vec![rel("Nouveau-ne", "age", "0"), rel("Doyen", "age", "130")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_just_outside_boundary_rejected() {
        let relations = vec![rel("X", "age", "130.001")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_non_numeric_object_for_numeric_predicate_rejected() {
        let relations = vec![rel("X", "age", "tres vieux")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
        assert!(report.violations[0].reason.contains("non numerique"));
    }

    #[test]
    fn test_implausible_height_rejected() {
        let relations = vec![rel("X", "height_cm", "500")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
        assert_eq!(report.violations[0].predicate, "height_cm");
    }

    #[test]
    fn test_plausible_height_passes() {
        let relations = vec![rel("X", "height_cm", "180")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_implausible_weight_rejected() {
        let relations = vec![rel("X", "weight_kg", "5000")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_implausible_distance_rejected() {
        // Superieur a la distance antipodale maximale sur Terre -- ne peut
        // pas exister comme distance directe entre deux lieux terrestres.
        let relations = vec![rel("Paris", "distance_km", "50000")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_plausible_distance_passes() {
        let relations = vec![rel("Paris", "distance_km", "343")]; // Paris-Londres, ordre de grandeur
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_implausible_temperature_rejected() {
        let relations = vec![rel("Sahara", "temperature_celsius", "100")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_plausible_temperature_passes() {
        let relations = vec![rel("Sahara", "temperature_celsius", "45")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_unrelated_predicate_ignored() {
        // "capital_of" n'est pas dans NUMERIC_BOUNDS -- ce module ne doit
        // jamais essayer de le parser en nombre.
        let relations = vec![rel("France", "capital_of", "Paris")];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    // ── check_lifespan_plausibility ──

    #[test]
    fn test_plausible_lifespan_passes() {
        let relations = vec![
            rel("Marie Curie", "birth_year", "1867"),
            rel("Marie Curie", "death_year", "1934"),
        ];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(report.plausible);
    }

    #[test]
    fn test_death_before_birth_rejected() {
        let relations = vec![
            rel("X", "birth_year", "2000"),
            rel("X", "death_year", "1990"),
        ];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
        assert!(report.violations.iter().any(|v| v.reason.contains("negative")));
    }

    #[test]
    fn test_lifespan_over_max_rejected() {
        let relations = vec![
            rel("X", "birth_year", "1800"),
            rel("X", "death_year", "2000"), // 200 ans
        ];
        let report = check_domain_plausibility(&relations, &[]);
        assert!(!report.plausible);
    }

    #[test]
    fn test_lifespan_split_across_history_and_new_payload_is_checked() {
        // birth_year dans l'historique, death_year dans le nouveau payload
        // -- le check combine doit quand meme attraper une incoherence.
        let history = vec![rel("X", "birth_year", "1990")];
        let new_relations = vec![rel("X", "death_year", "1980")]; // avant la naissance
        let report = check_domain_plausibility(&new_relations, &history);
        assert!(!report.plausible);
    }

    #[test]
    fn test_lifespan_entirely_in_history_not_reexamined() {
        // Meme principe de deduplication que execution_lab.rs: une paire
        // deja entierement dans l'historique (deja signalee a l'epoque) ne
        // doit pas ressurgir a chaque requete future sans rapport.
        let history = vec![
            rel("X", "birth_year", "2000"),
            rel("X", "death_year", "1990"),
        ];
        let new_relations = vec![rel("Y", "age", "40")];
        let report = check_domain_plausibility(&new_relations, &history);
        assert!(report.plausible, "violations inattendues: {:?}", report.violations);
    }

    #[test]
    fn test_relevant_predicates_matches_numeric_bounds_plus_lifespan() {
        let preds = relevant_predicates();
        assert_eq!(preds.len(), NUMERIC_BOUNDS.len() + 2);
        assert!(preds.contains(&"age"));
        assert!(preds.contains(&"birth_year"));
        assert!(preds.contains(&"death_year"));
    }
}
