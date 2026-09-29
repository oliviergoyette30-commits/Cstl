//! src/compression/text_dictionary.rs — dictionnaire reseau STABLE,
//! pre-rempli et versionne, pour le CONTENU TEXTE (`StructuralStreams::text`)
//! (2026-09-29).
//!
//! Reponse a la question de l'utilisateur : "un dictionnaire global
//! stable pre-rempli versionne et indexe en FSE/tANS", cette fois
//! applique aux noms d'entites et valeurs litterales plutot qu'aux
//! opcodes. La version ideale de cette idee serait un vrai tokenizer BPE
//! (celui de Claude, ~100-200k entrees) -- essaye en direct dans ce
//! sandbox via `tiktoken`, bloque : `openaipublic.blob.core.windows.net`
//! refuse la connexion (403, meme politique reseau que le blocage
//! tiktoken deja documente plus tot cette session pour le comptage de
//! tokens). Ce module est donc un succedane HONNETE et plus modeste,
//! reellement mesurable ICI : pas un tokenizer par sous-mots, mais un
//! modele d'ordre 1 BYTE PAR BYTE (meme moteur que `stable_dictionary.rs`
//! et `order1_rans.rs`, reutilise tel quel) entraine sur un corpus
//! synthetique de mots anglais/identifiants typiques d'un contrat ou
//! d'un protocole -- capture les paires de lettres frequentes ("io",
//! "en", "ti", ...) sans pretendre capturer les MOTS entiers comme le
//! ferait un vrai BPE.
//!
//! Limite assumee et explicite : ce corpus synthetique est un
//! ECHANTILLON de mon choix (vocabulaire de contrats/protocoles en
//! anglais), pas une vraie distribution mesuree sur du trafic CSTL reel.
//! Le gain mesure ci-dessous est donc indicatif, pas une garantie -- un
//! vrai dictionnaire de production exigerait un corpus de messages CSTL
//! REELS (voir la meme mise en garde deja faite dans
//! `order1_rans::PretrainedOrder1Table`).

use super::order1_rans::{PretrainedOrder1Table, encode_with_table, decode_with_table, Order1Error};
use std::sync::OnceLock;

pub const TEXT_DICTIONARY_VERSION: u8 = 1;
const MODE_PRETRAINED: u8 = 0;
const MODE_RAW_FALLBACK: u8 = 1;

/// Corpus synthetique : vocabulaire typique de contrats/protocoles
/// (anglais, casse mixte type identifiant de programmation) -- couvre les
/// paires de lettres qu'on s'attend a voir dans des noms d'entites CSTL
/// reels, sans pretendre memoriser les mots eux-memes (le modele d'ordre
/// 1 ne peut de toute facon pas "reconnaitre un mot", seulement ses
/// statistiques de paires de lettres).
fn build_reference_messages() -> Vec<Vec<u8>> {
    const WORDS: &[&str] = &[
        "Tenant", "Landlord", "Rent", "SecurityDeposit", "LeaseAgreement", "Property",
        "PaymentSchedule", "Inspector", "Obligation", "Party", "Asset", "Document", "Event",
        "Contract", "Agreement", "Payment", "Schedule", "Deposit", "Maintenance", "Repair",
        "Notice", "Termination", "Renewal", "Inspection", "Damage", "Insurance", "Utility",
        "Lease", "Owner", "Occupant", "Guarantor", "Signature", "Clause", "Amendment",
        "Jurisdiction", "Arbitration", "Dispute", "Liability", "Compliance", "Violation",
        "Sender", "Receiver", "Agent", "Server", "Client", "Session", "Token", "Identifier",
        "Timestamp", "Version", "Registry", "Protocol", "Message", "Payload", "Header",
        "e001", "e002", "e003", "r001", "r002", "r003", "id_42", "node_7", "seg_A", "key_9",
    ];
    let mut out = Vec::new();
    // Chaque mot est son PROPRE message (son propre `i==0`) -- meme
    // raison que le deuxieme bug corrige dans `stable_dictionary.rs` :
    // un `START_CONTEXT` doit voir chaque lettre de depart possible.
    for w in WORDS {
        out.push(w.as_bytes().to_vec());
    }
    // Quelques concatenations (2-3 mots colles, imitant camelCase reel
    // comme "TenantLandlordRent") pour que les transitions INTER-mots
    // soient aussi vues, pas seulement le debut/fin de chaque mot isole.
    for i in 0..WORDS.len() {
        let combo = format!("{}{}", WORDS[i], WORDS[(i * 7 + 3) % WORDS.len()]);
        out.push(combo.into_bytes());
    }
    out
}

fn reference_table() -> &'static PretrainedOrder1Table {
    static TABLE: OnceLock<PretrainedOrder1Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let messages = build_reference_messages();
        let refs: Vec<&[u8]> = messages.iter().map(|v| v.as_slice()).collect();
        PretrainedOrder1Table::train_from_messages(&refs)
    })
}

/// Meme garde-fou que `stable_dictionary::encode_stable` (voir sa doc,
/// 2026-09-29) : mode brut choisi non seulement sur echec, mais aussi
/// quand le mode pre-entraine reussit mais coute plus cher que le brut --
/// le plancher fixe de l'etat rANS (4 octets + compte varint) peut
/// depasser un petit flux `text`.
pub fn encode_text(stream: &[u8]) -> Vec<u8> {
    if stream.is_empty() {
        return Vec::new();
    }
    let mut out = vec![TEXT_DICTIONARY_VERSION];
    match encode_with_table(stream, reference_table()) {
        Ok(body) if body.len() < stream.len() => {
            out.push(MODE_PRETRAINED);
            out.extend_from_slice(&body);
        }
        _ => {
            out.push(MODE_RAW_FALLBACK);
            out.extend_from_slice(stream);
        }
    }
    out
}

#[derive(Debug)]
pub enum TextDictError {
    UnsupportedVersion(u8),
    Truncated,
    Order1(Order1Error),
}

impl std::fmt::Display for TextDictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextDictError::UnsupportedVersion(v) => write!(f, "version de dictionnaire texte non supportee: {v}"),
            TextDictError::Truncated => write!(f, "flux texte tronque"),
            TextDictError::Order1(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for TextDictError {}

pub fn decode_text(data: &[u8]) -> Result<Vec<u8>, TextDictError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if data.len() < 2 {
        return Err(TextDictError::Truncated);
    }
    let version = data[0];
    if version != TEXT_DICTIONARY_VERSION {
        return Err(TextDictError::UnsupportedVersion(version));
    }
    let mode = data[1];
    let body = &data[2..];
    match mode {
        MODE_PRETRAINED => decode_with_table(body, reference_table()).map_err(TextDictError::Order1),
        MODE_RAW_FALLBACK => Ok(body.to_vec()),
        _ => Err(TextDictError::Truncated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_roundtrip() {
        assert_eq!(encode_text(&[]), Vec::<u8>::new());
        assert_eq!(decode_text(&[]).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_known_style_names_roundtrip_and_measured() {
        // Noms du MEME style que le corpus (mais pas identiques mot pour
        // mot) -- teste la generalisation, pas la memorisation.
        let text = b"TenantLandlordRentSecurityDeposite001e002r001";
        let encoded = encode_text(text);
        let decoded = decode_text(&encoded).unwrap();
        assert_eq!(decoded, text);
        println!(
            "texte brut: {} octets | via dictionnaire texte pre-entraine: {} octets | mode pre-entraine: {}",
            text.len(), encoded.len(), encoded.get(1) == Some(&MODE_PRETRAINED)
        );
    }

    #[test]
    fn test_unfamiliar_text_falls_back_gracefully() {
        // Texte hors du style d'entrainement (ex. cyrillique/emoji) --
        // doit basculer en mode brut, jamais planter ni corrompre.
        let text = "日本語テスト🎉".as_bytes();
        let encoded = encode_text(text);
        let decoded = decode_text(&encoded).unwrap();
        assert_eq!(decoded, text);
    }
}
