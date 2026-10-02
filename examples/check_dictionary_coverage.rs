/// examples/check_dictionary_coverage.rs -- repond a la question
/// d'Olivier (2026-10-02): "dans le payload [du benchmark wire-format],
/// quels mots sont non couverts [par le dictionnaire texte] ?"
///
/// Mesure directe, pas une lecture de la liste WORDS de
/// text_dictionary.rs a l'oeil -- le dictionnaire marche au niveau
/// BIGRAMME D'OCTETS (ordre 1), pas mot-par-mot, donc un mot absent de
/// la liste WORDS peut quand meme etre bien compresse si ses paires de
/// lettres ressemblent au corpus d'entrainement, et un mot present peut
/// quand meme couter plus cher si le contexte autour change. On teste
/// donc chaque mot individuellement avec encode_text (le vrai code),
/// pas une recherche dans WORDS.
use cstl_parser::compression::text_dictionary::encode_text;

fn test_word(w: &str) -> (usize, usize, bool) {
    let bytes = w.as_bytes();
    let encoded = encode_text(bytes);
    let pretrained = encoded.get(1) == Some(&0u8); // MODE_PRETRAINED = 0
    (bytes.len(), encoded.len(), pretrained)
}

fn main() {
    // Mots EXACTS tires du payload reel de benchmark_wire_formats.rs
    // (celui qui a servi a publier la comparaison CSTL/JSON/Protobuf/
    // texte naturel dans le README) -- identifiants techniques du
    // benchmark + vocabulaire francais de la prose "texte naturel".
    let benchmark_words: Vec<&str> = vec![
        // identifiants/valeurs techniques du payload CSTL/JSON/Protobuf
        "concept_0", "concept_5", "concept_19", "entity", "implies",
        "wirefmt_bench_agent", "server", "inform", "wire_format_comparison",
        // vocabulaire francais de la version "texte naturel" du meme contenu
        "Une", "definition", "repetitive", "pour", "tester", "la",
        "compression", "du", "terme", "Message", "sujet", "comparaison",
        "formats", "concepts", "connaitre", "relations", "entre", "ces",
        // pour comparaison : mots REELLEMENT dans la liste WORDS du dictionnaire
        "Tenant", "Landlord", "Agent", "Server", "Message", "Payload",
    ];

    println!("=== Couverture reelle du dictionnaire texte sur le payload de benchmark (mesure, pas liste) ===\n");
    println!("{:28} {:>6} {:>8} {:>12}", "mot", "brut", "encode", "mode");
    let mut n_pretrained = 0;
    let mut n_raw = 0;
    for w in &benchmark_words {
        let (raw_len, enc_len, pretrained) = test_word(w);
        let mode = if pretrained { "PRETRAINED" } else { "RAW_FALLBACK" };
        if pretrained { n_pretrained += 1 } else { n_raw += 1 }
        println!("{w:28} {raw_len:>6}o {enc_len:>8}o {mode:>12}");
    }
    println!("\n{n_pretrained} mots en mode PRETRAINED, {n_raw} mots en mode RAW_FALLBACK sur {} testes", benchmark_words.len());

    // Test sur le flux TEXTE COMPLET concatene (plus representatif qu'un mot isole
    // -- le modele d'ordre 1 beneficie du contexte inter-mots)
    let full_french_prose = "Une definition repetitive pour tester la compression du terme Message sujet comparaison formats concepts connaitre relations entre ces";
    let full_ids = "concept_0concept_1concept_2concept_3concept_4wirefmt_bench_agentserverwire_format_comparison";
    // Test de controle : texte LONG compose UNIQUEMENT de mots du corpus
    // d'entrainement -- pour verifier si RAW_FALLBACK vient du vocabulaire
    // (mots absents) ou du plancher fixe de l'etat rANS sur les flux courts
    // (voir commentaire de encode_text : "le plancher fixe de l'etat rANS
    // peut depasser un petit flux text").
    let long_in_vocab = "TenantLandlordRentSecurityDepositLeaseAgreementPropertyPaymentScheduleInspectorObligationPartyAssetDocumentEventContractAgreementPaymentScheduleDepositMaintenanceRepairNoticeTerminationRenewalInspectionDamageInsuranceUtility";
    for (label, text) in [("prose francaise complete", full_french_prose), ("identifiants techniques concatenes", full_ids), ("LONG, 100% vocabulaire entraine", long_in_vocab)] {
        let bytes = text.as_bytes();
        let encoded = encode_text(bytes);
        let pretrained = encoded.get(1) == Some(&0u8);
        println!(
            "\n[{label}] brut={}o encode={}o mode={} ratio={:.1}%",
            bytes.len(), encoded.len(), if pretrained {"PRETRAINED"} else {"RAW_FALLBACK"},
            100.0 * encoded.len() as f64 / bytes.len() as f64
        );
    }
}
