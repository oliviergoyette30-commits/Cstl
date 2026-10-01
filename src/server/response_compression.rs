//! src/server/response_compression.rs — colle le codec generique
//! (`compression::response`) au format texte REEL des reponses construites
//! dans `handler.rs` (2026-10-01). Symetrique, cote REPONSES, de ce que
//! `parser.rs::record_compressed_payload` fait deja cote REQUETES pour le
//! bloc `COMPRESSED_PAYLOAD`.
//!
//! Point d'integration volontairement UNIQUE et minimal: `handler.rs`
//! construit deja ses reponses comme de simples `String` (format!/concat),
//! sur des dizaines de sites d'appel distincts -- recompresser CHAQUE site
//! individuellement aurait ete un changement de bien plus grande ampleur et
//! de bien plus grand risque. `maybe_compress_response` prend a la place la
//! `String` deja construite, juste avant le `socket.write_all(...)`, et la
//! remplace par une version compressee SI le client l'a demande (champ
//! `compress_response=true` dans son INTENT_PAYLOAD de requete) ET si la
//! reponse s'y prete de maniere PROUVABLEMENT sure -- voir la garde de
//! roundtrip ci-dessous. Chaque site d'appel ne change que d'UNE ligne:
//! `socket.write_all(response.as_bytes())` devient
//! `socket.write_all(maybe_compress_response(&response, want_compressed).as_bytes())`.
//!
//! Garde de securite (jamais de corruption, meme dans un cas limite non
//! anticipe): avant d'envoyer quoi que ce soit de compresse, le texte de la
//! reponse est RE-PARSE en blocs generiques puis RE-RENDU en texte, et
//! compare OCTET PAR OCTET a l'original. Si la moindre difference existe
//! (un champ contenant une virgule non geree, un format de bloc inattendu,
//! peu importe), la compression est simplement SAUTEE pour ce message --
//! le texte brut original part tel quel. Zero risque de corrompre une
//! reponse pour gagner de la bande passante.

use base64::Engine as _;

use crate::compression::response::{compress_response_blocks, ResponseBlock};
use super::parser::split_top_level_commas;

/// Nom du bloc wire qui remplace l'ENSEMBLE des blocs d'une reponse
/// compressee -- pendant unique de `COMPRESSED_PAYLOAD` cote requete, mais
/// un nom DIFFERENT delibere: la portee n'est pas la meme (ici N'IMPORTE
/// QUEL bloc de reponse, pas seulement defines/relations/uncertainty), et
/// un client qui ne sait decoder que l'un des deux ne doit jamais confondre
/// les deux formats.
pub const COMPRESSED_RESPONSE_BLOCK: &str = "COMPRESSED_RESPONSE";

/// Coupe une reponse `"#!CSTL ...\n<blocs>\n---END---\n"` en
/// (en-tete, corps, pied) -- `None` si la forme attendue n'est pas
/// respectee (ne devrait jamais arriver sur une reponse construite par ce
/// serveur, mais on ne suppose rien).
pub(crate) fn split_response(response: &str) -> Option<(&str, &str, &str)> {
    let header_end = response.find('\n')? + 1;
    let header = &response[..header_end];
    if !header.starts_with("#!CSTL ") {
        return None;
    }
    let footer_start = response.rfind("---END---")?;
    if footer_start < header_end {
        return None;
    }
    let body = &response[header_end..footer_start];
    let footer = &response[footer_start..];
    Some((header, body, footer))
}

/// Parse le corps (tout ce qui est entre l'en-tete et `---END---`) en une
/// liste ordonnee de blocs generiques. `None` si une ligne non-vide ne
/// respecte pas la forme `NOM [cle=valeur, ...]` -- traite comme
/// "non compressible en securite", jamais comme une erreur fatale (voir
/// `maybe_compress_response`).
fn parse_plain_blocks(body: &str) -> Option<Vec<ResponseBlock>> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bracket_start = line.find('[')?;
        let name = line[..bracket_start].trim();
        if name.is_empty() {
            return None;
        }
        let bracket_end = line.rfind(']')?;
        if bracket_end < bracket_start {
            return None;
        }
        let content = &line[bracket_start + 1..bracket_end];
        let mut fields = Vec::new();
        for part in split_top_level_commas(content) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part.split_once('=')?;
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
        out.push((name.to_string(), fields));
    }
    Some(out)
}

/// Inverse de `parse_plain_blocks` -- DOIT reproduire exactement le format
/// que `handler.rs` produit lui-meme (`NOM [cle=valeur, cle2=valeur2]\n`,
/// separateur ", "), puisque c'est precisement cette egalite octet-par-
/// octet qui sert de garde de securite avant d'envoyer une version
/// compressee.
fn render_plain_blocks(blocks: &[ResponseBlock]) -> String {
    let mut out = String::new();
    for (name, fields) in blocks {
        out.push_str(name);
        out.push_str(" [");
        let rendered: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
        out.push_str(&rendered.join(", "));
        out.push_str("]\n");
    }
    out
}

/// Tente de compresser `response` en un bloc unique `COMPRESSED_RESPONSE`.
/// `None` (jamais de panique) si quoi que ce soit dans la forme de la
/// reponse ne permet pas de garantir un roundtrip parfait.
fn try_compress(response: &str) -> Option<String> {
    let (header, body, footer) = split_response(response)?;
    let blocks = parse_plain_blocks(body)?;
    if blocks.is_empty() {
        return None; // rien a gagner, et evite l'overhead d'un bloc vide
    }
    if render_plain_blocks(&blocks) != body {
        return None; // garde de securite: pas de roundtrip prouve, on n'y touche pas
    }

    let compressed = compress_response_blocks(&blocks);
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let candidate = format!("{header}{COMPRESSED_RESPONSE_BLOCK} [data={data_b64}]\n{footer}");

    // Garde de TAILLE (2026-10-01, suite a la mesure live: 137%-152% de la
    // taille originale sur de vraies reponses -- base64 + text_dictionary
    // entraine sur le mauvais vocabulaire, voir README). Meme principe que
    // le repli deja en place dans stable_dictionary/text_dictionary: le
    // mode compresse n'est utilise QUE s'il est reellement plus petit que
    // l'original, jamais suppose. Compare le texte complet (header+bloc+
    // footer) a l'original -- pas seulement les octets compresses -- pour
    // inclure fidelement le cout du base64 et du bloc COMPRESSED_RESPONSE
    // lui-meme dans la comparaison.
    if candidate.len() >= response.len() {
        return None;
    }
    Some(candidate)
}

/// Point d'entree appele a chaque site `socket.write_all(response...)` de
/// `handler.rs`. `want_compressed` vient du `INTENT_PAYLOAD.compress_response`
/// de la REQUETE du client (voir handler.rs) -- jamais applique par defaut,
/// retrocompatibilite totale avec tout client existant qui n'envoie pas ce
/// champ (`cstl_client.py` ne l'envoie pas encore par defaut non plus).
/// Meme quand demande, ne compresse que si c'est reellement plus petit
/// (garde de taille dans `try_compress`) -- un client qui demande toujours
/// `compress_response=true` ne paie donc jamais le surcout mesure sur les
/// reponses typiques actuelles, il recoit juste du texte brut dans ce cas.
pub fn maybe_compress_response(response: &str, want_compressed: bool) -> String {
    if !want_compressed {
        return response.to_string();
    }
    try_compress(response).unwrap_or_else(|| response.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RESPONSE: &str = "#!CSTL v5.0.0 MODE=A\n\
META [encoder=CstlNativeServer, produced_by=Server, status=processed]\n\
INTENT_PAYLOAD [purpose=acknowledgement, sender=server, receiver=alice]\n\
RELATION [type=received, subject=test, status=valid]\n\
GOVERNANCE [sender=alice, circuit=closed, breaker_trips=0]\n\
AUDIT [hash=sha256:03d86bf1, parent_hash=root, seq=0]\n\
---END---\n";

    #[test]
    fn test_want_compressed_false_leaves_response_untouched() {
        let out = maybe_compress_response(SAMPLE_RESPONSE, false);
        assert_eq!(out, SAMPLE_RESPONSE);
    }

    #[test]
    fn test_small_realistic_response_hits_size_guard_stays_plain_text() {
        // Mesure live (2026-10-01, voir README): une reponse de cette taille
        // compresse PLUS GROSSE que l'original (base64 + text_dictionary
        // entraine sur le mauvais vocabulaire) -- la garde de taille doit
        // donc la laisser telle quelle, jamais lui faire payer ce surcout.
        let out = maybe_compress_response(SAMPLE_RESPONSE, true);
        assert_eq!(out, SAMPLE_RESPONSE, "la garde de taille doit rejeter un gain negatif");
    }

    #[test]
    fn test_large_repetitive_response_compresses_and_roundtrips() {
        // Contrairement a SAMPLE_RESPONSE (realiste mais petite -- rejetee
        // par la garde de taille ci-dessus), une reponse avec beaucoup de
        // blocs au vocabulaire tres repete donne au text_dictionary/
        // structure-delta de quoi vraiment gagner: ce test verifie que le
        // CHEMIN de compression (pas seulement la garde qui le bloque)
        // fonctionne et roundtrip correctement quand il s'active.
        // Valeur IDENTIQUE repetee (pas une variante par iteration) --
        // c'est precisement la redondance que text_dictionary/structure-delta
        // exploitent; une valeur legerement differente a chaque ligne (ex.
        // un numero d'iteration) detruit cette redondance et empeche tout
        // gain, voir la mesure honnete documentee dans le README.
        let mut body = String::new();
        for _ in 0..40 {
            body.push_str("SEMANTIC_WARNING [detail=W608: DEFINE non reference par aucune RELATION]\n");
        }
        let large_response = format!("#!CSTL v5.0.0 MODE=A\n{body}---END---\n");

        let out = maybe_compress_response(&large_response, true);
        assert_ne!(out, large_response, "devrait compresser -- vocabulaire tres repete");
        assert!(out.len() < large_response.len(), "devrait etre reellement plus petit: {} vs {}", out.len(), large_response.len());
        assert!(out.contains("COMPRESSED_RESPONSE [data="));

        // Rejoue le cote reception: extrait le base64, decompresse, confirme
        // qu'on retrouve exactement les memes blocs que l'original.
        let (header, body_out, footer) = split_response(&out).unwrap();
        assert_eq!(header, "#!CSTL v5.0.0 MODE=A\n");
        assert_eq!(footer, "---END---\n");
        let blocks = parse_plain_blocks(body_out).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, COMPRESSED_RESPONSE_BLOCK);
        let data_b64 = blocks[0].1.iter().find(|(k, _)| k == "data").map(|(_, v)| v.as_str()).unwrap();
        let compressed = base64::engine::general_purpose::STANDARD.decode(data_b64).unwrap();
        let decompressed = crate::compression::response::decompress_response_blocks(&compressed).unwrap();

        let original_blocks = parse_plain_blocks(
            split_response(&large_response).unwrap().1
        ).unwrap();
        assert_eq!(decompressed, original_blocks);
    }

    #[test]
    fn test_empty_body_response_not_compressed() {
        let empty = "#!CSTL v5.0.0 MODE=A\n---END---\n";
        let out = maybe_compress_response(empty, true);
        assert_eq!(out, empty, "rien a compresser -- doit rester tel quel");
    }

    #[test]
    fn test_response_with_quoted_comma_value_parses_and_renders_safely() {
        // Un champ cite contenant une virgule ne doit PAS casser le parsing
        // (split_top_level_commas le gere) -- teste directement
        // parse_plain_blocks/render_plain_blocks/compress_response_blocks
        // (pas maybe_compress_response: ce message est trop court pour
        // passer la garde de taille, ce n'est pas ce que ce test verifie).
        let response = "#!CSTL v5.0.0 MODE=A\n\
INTENT_PAYLOAD [purpose=validation_error, errors=\"E304: Missing sender, E305: Missing receiver\"]\n\
---END---\n";
        let (_, body, _) = split_response(response).unwrap();
        let blocks = parse_plain_blocks(body).unwrap();
        assert_eq!(render_plain_blocks(&blocks), body, "garde octet-par-octet: doit roundtrip");

        let compressed = compress_response_blocks(&blocks);
        let decompressed = crate::compression::response::decompress_response_blocks(&compressed).unwrap();
        assert_eq!(render_plain_blocks(&decompressed), body);
    }

    #[test]
    fn test_malformed_body_falls_back_to_plain_text_not_panic() {
        // Corps qui ne respecte pas la forme NOM [cle=valeur] -- doit
        // retomber sur le texte original, jamais paniquer ni corrompre.
        let weird = "#!CSTL v5.0.0 MODE=A\nceci n'est pas un bloc valide\n---END---\n";
        let out = maybe_compress_response(weird, true);
        assert_eq!(out, weird);
    }
}
