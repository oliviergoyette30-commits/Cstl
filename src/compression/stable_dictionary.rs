//! src/compression/stable_dictionary.rs — dictionnaire reseau STABLE,
//! pre-rempli et versionne, pour le flux d'opcodes de `structural.rs`
//! (2026-09-29).
//!
//! Reponse directe a la demande de l'utilisateur : "un dictionnaire
//! reseau stable pre-rempli versionne et un dictionnaire pour le reste".
//! Le "dictionnaire pour le reste" est deja `order1_rans::encode`
//! (table adaptative embarquee par message, existant) applique au flux
//! `StructuralStreams::variable`. CE module est le premier des deux :
//! une table d'ordre 1 entrainee UNE FOIS, a la compilation, sur un
//! corpus synthetique couvrant l'INTEGRALITE du vocabulaire ferme
//! (33 operateurs, 10 modalites, 4 statuts d'incertitude, les drapeaux
//! 0/1), et appliquee au flux `StructuralStreams::stable` sans JAMAIS
//! etre retransmise -- exactement le mecanisme deja prouve fonctionner
//! dans `order1_rans::tests` (contre-epreuve : memes octets qu'a
//! l'entrainement -> table non transmise, roundtrip correct), applique
//! maintenant a un flux qui est GENUINEMENT stable d'un message a
//! l'autre (contrairement au flux structurel complet d'avant la
//! separation, qui melangeait ca avec des index locaux -- voir le
//! commentaire d'echec dans `compression::master::tests::
//! test_pretrained_table_removes_per_message_overhead`).
//!
//! Versionnage : `STABLE_DICTIONARY_VERSION` est ecrit en tete de chaque
//! flux encode. Si ce module est un jour etendu (nouvel operateur,
//! nouvelle modalite), la version change et un decodeur qui ne connait
//! pas cette version peut le detecter explicitement plutot que de
//! decoder du bruit -- meme esprit que `server::wai::DictionaryVersion`
//! deja present dans ce depot pour le dictionnaire de mots-cles WAI,
//! applique ici a une table de probabilites plutot qu'a une liste de
//! symboles.
//!
//! Robustesse : si le flux `stable` d'un message reel contient un
//! enchainement d'octets jamais vu dans le corpus synthetique
//! d'entrainement (ne devrait pas arriver -- le corpus couvre tout le
//! vocabulaire ferme -- mais pas de garantie mathematique absolue sur
//! TOUTES les paires de contexte possibles), `encode_stable` bascule sur
//! un mode brut plutot que d'echouer -- un marqueur d'un octet en tete
//! du flux distingue les deux cas. Jamais de perte de donnees, jamais de
//! panique.

use super::order1_rans::{PretrainedOrder1Table, encode_with_table, decode_with_table, Order1Error};
use super::structural::encode_structural;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Incrementer si le vocabulaire ferme (OFFICIAL_OPERATORS,
/// VALID_CONSTRAINT_MODALITIES, VALID_UNCERTAINTY_STATUSES) change un
/// jour -- documente l'intention, pas encore applique a une verification
/// automatique (v1, meme portee assumee que le reste de ce module).
///
/// v1 -> v2 (2026-10-05) : ajout de `EITHER_OR` a `OFFICIAL_OPERATORS`
/// (37 -> 38 operateurs). `build_reference_messages` ci-dessous itere sur
/// `OFFICIAL_OPERATORS` dynamiquement, donc le corpus d'entrainement
/// couvre deja le nouvel operateur sans modification de ce fichier au-dela
/// de ce numero de version -- seul le numero change, pour qu'un decodeur
/// qui ne connaitrait que l'ancienne table detecte la divergence au lieu
/// de decoder du bruit silencieusement.
///
/// v2 -> v3 (2026-10-05) : ajout de `REACTS` a `OFFICIAL_OPERATORS`
/// (38 -> 39 operateurs), meme raisonnement -- `build_reference_messages`
/// couvre le nouvel operateur dynamiquement, seul le numero de version
/// change.
pub const STABLE_DICTIONARY_VERSION: u8 = 3;

const MODE_PRETRAINED: u8 = 0;
const MODE_RAW_FALLBACK: u8 = 1;

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Petit xorshift32 deterministe (graine fixe) -- pas de dependance
/// externe, pas `rand::random`/`Instant::now` (interdits dans un contexte
/// reproductible), juste un melange stable pour eviter que le corpus
/// d'entrainement place systematiquement les memes octets de contexte
/// avant chaque operateur (voir bug trouve et documente ci-dessous).
fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Construit le corpus synthetique d'entrainement : toutes les
/// combinaisons (operateur x modalite-ou-aucune x id present/absent),
/// MELANGEES (pas groupees par operateur) pour que chaque operateur soit
/// vu precede par les DEUX valeurs possibles du drapeau `has_id` (0 et 1)
/// de la relation precedente, pas seulement une.
///
/// Bug trouve en verifiant (2026-09-29), pas suppose corrige : une
/// premiere version generait les relations GROUPEES par operateur (toutes
/// les variantes d'un operateur, puis toutes celles du suivant). Chaque
/// groupe se terminait TOUJOURS par la meme relation (sans modalite, sans
/// id -- has_id=0), donc le contexte "octet precedent = 1" (has_id=1)
/// n'apparaissait JAMAIS juste avant un octet d'operateur -- seul le
/// contexte 0 y menait. Le message de test reel (`PERFORM ... [id=r001]`
/// suivi de `ARR.ACCESS ...`) a un has_id=1 juste avant ARR.ACCESS, donc
/// `UnknownSlot` a l'usage reel. Le melange ci-dessous corrige la cause,
/// pas le symptome.
/// Retourne une liste de MESSAGES INDEPENDANTS (pas un flux concatene) --
/// important : `context_for` definit `START_CONTEXT` comme "aucun octet
/// precedent", c'est-a-dire uniquement l'index 0 D'UNE SEQUENCE. Un
/// deuxieme bug trouve en verifiant (2026-09-29), distinct du premier :
/// une premiere version de cette fonction concatenait tous les messages
/// synthetiques en UN SEUL flux avant entrainement -- alors seul le tout
/// premier octet du corpus ENTIER voyait jamais `START_CONTEXT`, tous les
/// autres "debuts de message logiques" a l'interieur du flux concatene
/// n'etaient en realite que "l'octet apres la fin du message precedent",
/// un contexte different. Resultat : `UnknownSlot(256, 18)` -- le vrai
/// premier octet d'un message reel (operateur PERFORM) n'avait jamais ete
/// vu sous `START_CONTEXT`, meme apres avoir ajoute une passe par
/// operateur, parce que cette passe etait ELLE AUSSI concatenee au reste.
/// Fixe en entrainant desormais via `PretrainedOrder1Table::
/// train_from_messages` (chaque message garde son propre `i==0`) plutot
/// que `train` sur un flux aplati.
fn build_reference_messages() -> Vec<Vec<u8>> {
    use crate::semantic::OFFICIAL_OPERATORS;
    use crate::server::parser::{VALID_CONSTRAINT_MODALITIES, VALID_UNCERTAINTY_STATUSES};

    let entities = ["A", "B"]; // valeurs sans importance -- vont dans `variable`, pas `stable`
    let mut messages = Vec::new();

    let mut relations = Vec::new();
    for &op in OFFICIAL_OPERATORS {
        for &id_present in &[true, false] {
            for &modality in VALID_CONSTRAINT_MODALITIES {
                let mut attrs = vec![("type", op), ("subject", entities[0]), ("object", entities[1]), ("modality", modality)];
                if id_present { attrs.push(("id", "r1")); }
                relations.push(m(&attrs));
            }
            let mut attrs = vec![("type", op), ("subject", entities[0]), ("object", entities[1])];
            if id_present { attrs.push(("id", "r1")); }
            relations.push(m(&attrs));
        }
    }
    let mut seed: u32 = 0xC57A_1234u32.wrapping_add(1); // graine fixe arbitraire
    let n = relations.len();
    for i in (1..n).rev() {
        let r = (xorshift32(&mut seed) as usize) % (i + 1);
        relations.swap(i, r);
    }

    let mut uncertainty = Vec::new();
    for &status in VALID_UNCERTAINTY_STATUSES {
        uncertainty.push(m(&[("identifier", "e1"), ("status", status), ("sigma", "0.5")]));
        uncertainty.push(m(&[("identifier", "e1"), ("status", status)]));
    }
    for i in (1..uncertainty.len()).rev() {
        let r = (xorshift32(&mut seed) as usize) % (i + 1);
        uncertainty.swap(i, r);
    }

    // Ce message-ci (le gros, melange) couvre les transitions internes
    // (operateur -> drapeaux -> operateur suivant, etc.) -- c'est lui qui
    // fixait le PREMIER bug (has_id=1 jamais vu avant un operateur).
    messages.push(encode_structural(&[], &relations, &uncertainty).stable);

    let mut relations2 = relations.clone();
    for i in (1..n).rev() {
        let r = (xorshift32(&mut seed) as usize) % (i + 1);
        relations2.swap(i, r);
    }
    messages.push(encode_structural(&[], &relations2, &uncertainty).stable);

    // Un message DEDIE par operateur, chacun avec CET operateur comme
    // TOUT PREMIER octet -- couvre `START_CONTEXT -> chaque operateur`,
    // le deuxieme bug. Distinct des gros messages ci-dessus : chacun de
    // ceux-ci est sa PROPRE sequence (son propre `i==0`), pas concatene.
    for &op in OFFICIAL_OPERATORS {
        let relations = vec![m(&[("type", op), ("subject", entities[0]), ("object", entities[1])])];
        messages.push(encode_structural(&[], &relations, &[]).stable);
        // Variante avec modalite, pour aussi couvrir START -> operateur
        // suivi d'un contexte "has_modality=1" different de l'exemple nu.
        let relations_mod = vec![m(&[("type", op), ("subject", entities[0]), ("object", entities[1]), ("modality", "MUST"), ("id", "r1")])];
        messages.push(encode_structural(&[], &relations_mod, &[]).stable);
    }
    // Meme couverture START pour un message qui commence directement par
    // UNCERTAINTY plutot que par une relation.
    for &status in VALID_UNCERTAINTY_STATUSES {
        let uncertainty = vec![m(&[("identifier", "e1"), ("status", status)])];
        messages.push(encode_structural(&[], &[], &uncertainty).stable);
    }

    messages
}

fn reference_table() -> &'static PretrainedOrder1Table {
    static TABLE: OnceLock<PretrainedOrder1Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let messages = build_reference_messages();
        let refs: Vec<&[u8]> = messages.iter().map(|v| v.as_slice()).collect();
        PretrainedOrder1Table::train_from_messages(&refs)
    })
}

/// Encode le flux `StructuralStreams::stable` avec le dictionnaire
/// pre-entraine, JAMAIS retransmis -- seuls [version][mode][corps]
/// voyagent. Retombe sur un mode brut soit si le corpus de reference ne
/// couvre pas une transition presente dans ce message precis (jamais
/// d'echec, voir doc de module), SOIT si le mode pre-entraine reussit
/// mais produit plus gros que le brut -- l'etat rANS embarque un plancher
/// fixe (4 octets d'etat + 1-2 octets de compte varint) qui peut depasser
/// un flux `stable` minuscule (2026-09-29, reponse a "pour les petits
/// messages du bit-stream" -- mesure, pas suppose : voir test
/// `test_tiny_message_picks_smaller_of_pretrained_or_raw`).
pub fn encode_stable(stream: &[u8]) -> Vec<u8> {
    if stream.is_empty() {
        return Vec::new();
    }
    let mut out = vec![STABLE_DICTIONARY_VERSION];
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
pub enum StableDictError {
    Empty,
    UnsupportedVersion(u8),
    Truncated,
    Order1(Order1Error),
}

impl std::fmt::Display for StableDictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StableDictError::Empty => write!(f, "flux stable vide"),
            StableDictError::UnsupportedVersion(v) => write!(f, "version de dictionnaire stable non supportee: {v}"),
            StableDictError::Truncated => write!(f, "flux stable tronque"),
            StableDictError::Order1(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for StableDictError {}

pub fn decode_stable(data: &[u8]) -> Result<Vec<u8>, StableDictError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if data.len() < 2 {
        return Err(StableDictError::Truncated);
    }
    let version = data[0];
    if version != STABLE_DICTIONARY_VERSION {
        return Err(StableDictError::UnsupportedVersion(version));
    }
    let mode = data[1];
    let body = &data[2..];
    match mode {
        MODE_PRETRAINED => decode_with_table(body, reference_table()).map_err(StableDictError::Order1),
        MODE_RAW_FALLBACK => Ok(body.to_vec()),
        _ => Err(StableDictError::Truncated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::structural::encode_structural;
    use std::collections::HashMap;

    fn mm(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_empty_roundtrip() {
        assert_eq!(encode_stable(&[]), Vec::<u8>::new());
        assert_eq!(decode_stable(&[]).unwrap(), Vec::<u8>::new());
    }

    /// LE test qui repond a la question de l'utilisateur : le meme petit
    /// message qui avait echoue avec une table entrainee sur un corpus
    /// ad hoc reussit maintenant, avec zero octet de table transmis, en
    /// utilisant le dictionnaire de reference construit sur TOUT le
    /// vocabulaire ferme plutot que sur un echantillon partiel.
    #[test]
    fn test_real_message_roundtrips_via_reference_dictionary_zero_table_overhead() {
        let relations = vec![
            mm(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
            mm(&[("type", "ARR.ACCESS"), ("subject", "Tenant"), ("object", "SecurityDeposit"), ("modality", "MUST_NOT"), ("id", "r002")]),
        ];
        let streams = encode_structural(&[], &relations, &[]);
        assert!(!streams.stable.is_empty());

        let encoded = encode_stable(&streams.stable);
        let decoded = decode_stable(&encoded).unwrap();
        assert_eq!(decoded, streams.stable);

        let used_pretrained_mode = encoded[1] == MODE_PRETRAINED;
        println!(
            "flux stable brut: {} octets | encode via dictionnaire reference: {} octets | mode pre-entraine utilise: {}",
            streams.stable.len(), encoded.len(), used_pretrained_mode
        );
        assert!(used_pretrained_mode, "le corpus de reference doit couvrir ce message reel -- sinon la couverture du corpus synthetique est insuffisante, a corriger, pas a contourner");
    }

    /// Reponse a "pour les petits messages du bit-stream" (2026-09-29) :
    /// un flux `stable` minuscule (une seule relation nue, sans modalite
    /// ni id) est assez court pour que le PLANCHER FIXE de l'etat rANS
    /// (4 octets d'etat + compte varint, voir `encode_with_table`) coute
    /// plus cher que de transmettre le flux brut tel quel -- le garde-fou
    /// de taille dans `encode_stable` doit alors choisir le mode brut
    /// MEME QUAND le mode pre-entraine reussit, pas seulement en cas
    /// d'echec.
    #[test]
    fn test_tiny_message_picks_smaller_of_pretrained_or_raw() {
        let relations = vec![mm(&[("type", "PERFORM"), ("subject", "A"), ("object", "B")])];
        let streams = encode_structural(&[], &relations, &[]);
        assert!(!streams.stable.is_empty());

        let pretrained_body = encode_with_table(&streams.stable, reference_table()).unwrap();
        let encoded = encode_stable(&streams.stable);
        let decoded = decode_stable(&encoded).unwrap();
        assert_eq!(decoded, streams.stable);

        println!(
            "flux stable minuscule: {} octets brut | {} octets si mode pre-entraine force | {} octets choisis (mode {}) ",
            streams.stable.len(), pretrained_body.len(), encoded.len(),
            if encoded[1] == MODE_PRETRAINED { "pre-entraine" } else { "brut" }
        );
        // Le garde-fou doit toujours produire au moins aussi petit que
        // "toujours forcer le mode pre-entraine quand il reussit".
        assert!(encoded.len() <= 2 + pretrained_body.len().min(streams.stable.len()));
        if pretrained_body.len() >= streams.stable.len() {
            assert_eq!(encoded[1], MODE_RAW_FALLBACK, "le plancher fixe de l'etat rANS depasse le brut ici -- le mode brut doit gagner");
        }
    }

    #[test]
    fn test_unknown_operator_and_modality_fallback_still_roundtrips() {
        // Meme avec des operateurs/modalites HORS vocabulaire (mode
        // fallback 0xFF cote structural.rs), le flux stable ne contient
        // que des flags/valeurs deja couvertes par le corpus -- verifie
        // que ca reste correct.
        let relations = vec![mm(&[("type", "INVENTE"), ("subject", "A"), ("object", "B"), ("modality", "AUSSI_INVENTE")])];
        let streams = encode_structural(&[], &relations, &[]);
        let encoded = encode_stable(&streams.stable);
        let decoded = decode_stable(&encoded).unwrap();
        assert_eq!(decoded, streams.stable);
    }

    #[test]
    fn test_bad_version_rejected_explicitly() {
        let mut encoded = encode_stable(&[5, 1, 3, 0]);
        encoded[0] = 99; // version bidon
        assert!(matches!(decode_stable(&encoded), Err(StableDictError::UnsupportedVersion(99))));
    }

    #[test]
    fn test_larger_realistic_corpus_all_pretrained_zero_overhead() {
        // Corpus plus large, vocabulaire varie -- confirme que ce n'est
        // pas juste le petit exemple ci-dessus qui marche par chance.
        let ops = ["PERFORM", "ARR.ACCESS", "MAINTAIN", "TRANSMIT_FAITHFUL", "COMMAND", "ENTAILS", "BEFORE", "OPPOSES", "KNOWS", "CONTRADICTS"];
        let modalities = ["MUST", "MUST_NOT", "MAY", "SHOULD", "REQUIRE"];
        let mut relations = Vec::new();
        for i in 0..20usize {
            relations.push(mm(&[
                ("type", ops[i % ops.len()]),
                ("subject", "X"), ("object", "Y"),
                ("modality", modalities[i % modalities.len()]),
                ("id", "r"),
            ]));
        }
        let uncertainty = vec![
            mm(&[("identifier", "e1"), ("status", "ESTIMATED"), ("sigma", "0.5")]),
            mm(&[("identifier", "e2"), ("status", "UNKNOWN")]),
            mm(&[("identifier", "e3"), ("status", "MEASURED"), ("sigma", "0.1")]),
        ];
        let streams = encode_structural(&[], &relations, &uncertainty);
        let encoded = encode_stable(&streams.stable);
        let decoded = decode_stable(&encoded).unwrap();
        assert_eq!(decoded, streams.stable);
        assert_eq!(encoded[1], MODE_PRETRAINED, "doit rester en mode pre-entraine, zero table transmise, sur un corpus varie realiste");
        println!(
            "corpus realiste plus large -- flux stable brut: {} octets | via dictionnaire reference: {} octets (2 octets d'entete version+mode)",
            streams.stable.len(), encoded.len()
        );
    }
}
