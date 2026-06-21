// CSTL v4.9.3 — Ontologie des domaines
// Porte l'essentiel de cstl_domains.py (18 domaines) vers Rust.
// Utilisé par R5 pour accepter les opérateurs hors-21-officiels
// quand un DOMAIN est déclaré dans META.

use std::collections::HashSet;

pub fn domain_operators(domain: &str) -> HashSet<&'static str> {
    match domain.to_lowercase().as_str() {
        "diplomatique" => [
            "NEGOCIER", "RATIFIER", "SIGNER", "DIVULGUER", "DIVULGUE",
            "SANCTIONNER", "MEDIER", "PROTESTER", "RECONNAITRE",
            "OBTENIR", "OBTAIN", "EXPULSER", "RAPPELER",
        ].iter().copied().collect(),
        "juridique" => [
            "CONTESTER", "RESILIER", "NOTIFIER", "ESTER", "PLAIDER",
            "CONDAMNER", "ACQUITTER", "DIVULGUER", "DIVULGUE", "SIGNER",
            "OBTENIR", "OBTAIN", "MANDATER", "RECLAMER",
        ].iter().copied().collect(),
        "medical" | "médical" => [
            "PRESCRIRE", "DIAGNOSTIQUER", "CONTRA_INDIQUER", "ADMINISTRER",
            "OPERER", "SURVEILLER", "REFERER", "HOSPITALISER", "TRAITER",
        ].iter().copied().collect(),
        "cyber_securite" | "cyber" => [
            "BREACH", "PATCH", "MONITOR", "ISOLATE", "ESCALATE",
            "ENCRYPT", "DECRYPT", "AUTHENTICATE", "AUTHORIZE", "AUDIT",
            "ALERT", "BLOCK", "SCAN", "REMEDIATE",
        ].iter().copied().collect(),
        "finance" => [
            "FINANCER", "AUDITER", "PROVISIONNER", "COMPTABILISER",
            "REMBOURSER", "GARANTIR", "COUVRIR", "LIQUIDER",
            "VALORISER", "CONSOLIDER",
        ].iter().copied().collect(),
        "rh" => [
            "RECRUTER", "EVALUER", "LICENCIER", "PROMOUVOIR", "FORMER",
            "MUTER", "REMUNERER", "SANCTIONNER", "INTEGRER", "OFFBOARDER",
        ].iter().copied().collect(),
        "supply_chain" => [
            "LIVRER", "ROUTER", "BLOQUER", "SOURCER", "TRACER",
            "STOCKER", "EXPEDIER", "RECEPTIONNER", "RETOURNER",
            "APPROUVER", "COMMANDER",
        ].iter().copied().collect(),
        "education" => [
            "ENSEIGNER", "EVALUER", "CERTIFIER", "ORIENTER", "INSCRIRE",
            "EXCLURE", "DELIBERER", "VALIDER", "NOTER",
        ].iter().copied().collect(),
        "journalisme" => [
            "SOURCER", "VERIFIER", "PUBLIER", "RECTIFIER", "ENQUETER",
            "CITER", "REVELER", "DEMENTIR", "COMMENTER",
        ].iter().copied().collect(),
        "energie" => [
            "PRODUIRE", "DISTRIBUER", "STOCKER", "TARIFER", "CONNECTER",
            "DECONNECTER", "REGULER", "OPTIMISER", "PREVOIR",
        ].iter().copied().collect(),
        "recherche" => [
            "HYPOTHESER", "VALIDER", "REFUTER", "PUBLIER", "CITER",
            "REPRODUIRE", "EXPERIMENTER", "PEER_REVIEW",
        ].iter().copied().collect(),
        "gouvernance" | "compliance" => [
            "AUDITER", "CERTIFIER", "NOTIFIER", "SANCTIONNER",
            "REPORTER", "APPROUVER", "REJETER", "ESCALADER",
        ].iter().copied().collect(),
        "corporate" => ["APPROUVER","REJETER","DELEGUER","REPORTER","BUDGETER",
            "AUDITER","FUSIONNER","ACQUERIR","LICENCIER","RECRUTER","EVALUER"].iter().cloned().collect(),
        "astronomique" => ["OBSERVER","DETECTER","MESURER","CATALOGUER","NOMMER",
            "CONFIRMER","REFUTER","PUBLIER","SIMULER"].iter().cloned().collect(),
        "archeologique" | "archéologique" => ["DECOUVRIR","FOUILLER","DATER","CATALOGUER","PRESERVER",
            "PUBLIER","CONTESTER","ATTRIBUER","RESTAURER","EXCAVER"].iter().cloned().collect(),
        "reglementaire" => ["CERTIFIER","SANCTIONNER","NOTIFIER","ABROGER","HOMOLOGUER",
            "CONTROLER","AUTORISER","INTERDIRE","DECLARER","AUDITER","CONFORMER","REPORTER"].iter().cloned().collect(),
        "marketing" => ["CIBLER","SEGMENTER","CONVERTIR","FIDELISER","ACTIVER",
            "DESACTIVER","PERSONNALISER","MESURER","TESTER","OPTIMISER"].iter().cloned().collect(),
        "immobilier" => ["ACQUERIR","LOUER","HYPOTHEQUER","EVALUER","VENDRE",
            "GERER","RENOVER","RESILIER","NOTARIER"].iter().cloned().collect(),
        "assurance" => ["SOUSCRIRE","INDEMNISER","RESILIER","EXPERTISER","DECLARER",
            "COUVRIR","EXCLURE","REMBOURSER","EVALUER"].iter().cloned().collect(),
        "financier" => ["INVESTIR","ARBITRER","COUVRIR","LIQUIDER","LEVER",
            "REMBOURSER","AUDITER","CONSOLIDER","PROVISIONNER"].iter().cloned().collect(),
        _ => HashSet::new(),
    }
}

pub fn is_known_domain(domain: &str) -> bool {
    matches!(domain.to_lowercase().as_str(),
        "diplomatique" | "juridique" | "medical" | "médical" |
        "cyber_securite" | "cyber" | "finance" | "rh" |
        "supply_chain" | "education" | "journalisme" | "energie" |
        "recherche" | "gouvernance" | "compliance" | "general"
    )
}
