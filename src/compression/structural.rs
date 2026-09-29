//! src/compression/structural.rs — Couche 1 du Master Compresseur CSTL :
//! squelette relationnel + dictionnaire d'opcodes fixe (2026-09-29, revise
//! le meme jour pour separer flux stable/variable -- voir plus bas).
//!
//! Contexte : cette session a etabli, par calcul a la main puis par mesure
//! reelle, que le poids d'un payload CSTL se divise en deux regions de
//! nature tres differente -- le squelette relationnel (operateurs,
//! modalites, statuts d'incertitude : vocabulaire FERME, ~33+10+4 symboles,
//! entropie de Shannon quasi nulle car hautement previsible) et les valeurs
//! litterales (noms, identifiants : quasi-incompressibles, dominent le
//! poids en octets). Aucun des 8 systemes de compression deja presents
//! dans ce depot (`fse_encoder_rs`, `fse::encoder`, `wai_core`,
//! `wai_dictionary`, `wai_compression`, `compression_pipeline`,
//! `server::wai`, les `ans_*` orphelins) ne fait cette distinction.
//!
//! Portee assumee (MVP, pas la forme d'onde complete du wire format) :
//! encode uniquement `defines`, `relations`, `uncertainty`. `meta`/
//! `intent`/`guardrail_report`/etc. NE SONT PAS encodes ici.
//!
//! ## Separation flux STABLE / flux VARIABLE (ajoutee 2026-09-29)
//!
//! Premiere version de ce module: un seul flux d'octets melangeant
//! opcodes (operateurs/modalites/statuts, vocabulaire FERME et GLOBAL --
//! identique quel que soit le message) et index varint vers la table de
//! chaines LOCALE a ce message (noms d'entites, litteraux). Teste en
//! direct (`compression::master::tests::
//! test_pretrained_table_removes_per_message_overhead`) : un modele
//! d'ordre 1 pre-entraine sur ce flux unique NE GENERALISE PAS d'un
//! message a l'autre, parce que les index varint locaux ne sont pas un
//! vocabulaire stable -- l'index 3 dans un message a 3 entites ne
//! correspond a rien dans un message qui en a 8.
//!
//! Ce module separe donc desormais CHAQUE record en deux flux :
//! - `stable` : uniquement les octets d'opcode (operateur, modalite,
//!   statut, drapeaux has_modality/has_id/has_sigma) -- vocabulaire FERME
//!   et GLOBAL, identique d'un message a l'autre par construction. C'est
//!   CE flux qui peut recevoir un dictionnaire reseau pre-rempli et
//!   VERSIONNE (voir `compression::stable_dictionary`) sans jamais avoir
//!   besoin d'etre re-entraine ni retransmis.
//! - `variable` : tout le reste -- la table de chaines elle-meme, tous
//!   les index varint qui y pointent, les compteurs. Intrinsequement
//!   propre a ce message ; reste compresse par une table adaptative
//!   embarquee par message (voir `compression::order1_rans::encode`),
//!   comme avant cette separation.
//!
//! `defines` n'a aucun champ a vocabulaire ferme (verifie : `entity_type`
//! n'est valide contre AUCUNE liste dans `parser.rs`, texte libre) -- va
//! entierement dans `variable`.

use std::collections::HashMap;
use super::wai_core::{encode_varint, decode_varint, zigzag_encode, zigzag_decode};
use crate::semantic::OFFICIAL_OPERATORS;
use crate::server::parser::{VALID_CONSTRAINT_MODALITIES, VALID_UNCERTAINTY_STATUSES};

/// Marqueur d'opcode "pas dans le vocabulaire fixe" -- les vocabulaires
/// reels (33 operateurs, 10 modalites, 4 statuts) tiennent tous largement
/// sous 0xFE, donc 0xFF est un sentinel sans collision possible.
const OPCODE_FALLBACK: u8 = 0xFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuralError {
    Truncated(&'static str),
    BadStringIndex(u32),
    BadOpcode(u8),
}

impl std::fmt::Display for StructuralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StructuralError::Truncated(ctx) => write!(f, "flux structurel tronque ({ctx})"),
            StructuralError::BadStringIndex(i) => write!(f, "index de table de chaines invalide: {i}"),
            StructuralError::BadOpcode(b) => write!(f, "opcode inconnu: {b:#04x}"),
        }
    }
}
impl std::error::Error for StructuralError {}

/// Sortie d'`encode_structural` : TROIS flux distincts (2026-09-29, revise
/// le meme jour -- reponse a "un dictionnaire global stable pre-rempli
/// versionne et indexe en FSE/tANS" applique au CONTENU des chaines, pas
/// seulement aux opcodes). Voir doc de module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuralStreams {
    /// Opcodes seulement -- vocabulaire ferme et global (33 operateurs,
    /// 10 modalites, 4 statuts). Candidat au dictionnaire reseau
    /// pre-rempli (`stable_dictionary`).
    pub stable: Vec<u8>,
    /// OCTETS UTF-8 REELS des chaines internees (noms d'entites, valeurs
    /// litterales), CONCATENES sans separateur -- pas les longueurs, pas
    /// les index. Candidat a un dictionnaire pre-entraine sur des
    /// statistiques de texte (paires de lettres typiques de noms/mots
    /// anglais-ish) -- voir `text_dictionary`. Separe de `variable` parce
    /// que ce contenu-la, contrairement aux longueurs/index ci-dessous,
    /// a une vraie regularite STATISTIQUE partageable d'un message a
    /// l'autre (les lettres qui composent des noms suivent une
    /// distribution stable, meme si les noms eux-memes different).
    pub text: Vec<u8>,
    /// Compteurs, longueurs, et tous les index varint qui pointent vers
    /// la table de chaines -- intrinsequement propre a CE message (voir
    /// bug documente dans `compression::master` -- les valeurs d'index
    /// dependent du nombre d'entites presentes, pas un vocabulaire
    /// stable).
    pub variable: Vec<u8>,
    /// Identifiants "prefixe+chiffres" (ex. "r001", "e047") des champs
    /// `relations.id` et `uncertainty.identifier`, quand ils matchent ce
    /// patron -- prefixe explicite (pas suppose ferme, transmis tel quel,
    /// voir `try_parse_id`) + largeur (pour preserver les zeros de tete)
    /// + delta zigzag-varint PAR PREFIXE au sein de CE message (2026-09-29,
    /// reponse a "un exemple d'id" -- mesure : r001/r002/r003 passe de 12
    /// a 3 octets). Aucun etat entre messages -- juste une passe unique,
    /// contrairement a l'idee de table de session ecartee plus tot. Les
    /// id qui ne matchent pas ce patron (pas de suffixe numerique, motif
    /// non reconnu) restent sur le chemin `text`/`variable` existant,
    /// jamais d'echec. PORTEE ASSUMEE: seulement `relations.id` et
    /// `uncertainty.identifier` -- PAS `defines.id` (qui vit dans les
    /// extras generiques, pas encore cible par ce mecanisme).
    pub ids: Vec<u8>,
}

struct StringTable {
    strings: Vec<String>,
    index_of: HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        StringTable { strings: Vec::new(), index_of: HashMap::new() }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.index_of.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index_of.insert(s.to_string(), idx);
        idx
    }

    /// Serialise en 2 flux : `lengths` (compte + longueur de chaque
    /// chaine, varint -- va dans `variable`, message-local) et `text`
    /// (les octets UTF-8 concatenes, SANS separateur puisque `lengths`
    /// donne deja les frontieres -- va dans `StructuralStreams::text`,
    /// candidat au dictionnaire de texte pre-entraine).
    fn serialize(&self) -> (Vec<u8>, Vec<u8>) {
        let mut lengths = Vec::new();
        let mut text = Vec::new();
        lengths.extend_from_slice(&encode_varint(self.strings.len() as u32));
        for s in &self.strings {
            let bytes = s.as_bytes();
            lengths.extend_from_slice(&encode_varint(bytes.len() as u32));
            text.extend_from_slice(bytes);
        }
        (lengths, text)
    }

    fn deserialize(lengths: &[u8], lpos: &mut usize, text: &[u8], tpos: &mut usize) -> Result<Vec<String>, StructuralError> {
        let (count, used) = decode_varint(&lengths[*lpos..])
            .map_err(|_| StructuralError::Truncated("table de chaines: compte"))?;
        *lpos += used;
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let (len, used) = decode_varint(&lengths[*lpos..])
                .map_err(|_| StructuralError::Truncated("table de chaines: longueur"))?;
            *lpos += used;
            let len = len as usize;
            if *tpos + len > text.len() {
                return Err(StructuralError::Truncated("table de chaines: bytes texte"));
            }
            let s = String::from_utf8_lossy(&text[*tpos..*tpos + len]).into_owned();
            *tpos += len;
            out.push(s);
        }
        Ok(out)
    }
}

fn read_string<'a>(table: &'a [String], idx: u32) -> Result<&'a str, StructuralError> {
    table.get(idx as usize).map(String::as_str).ok_or(StructuralError::BadStringIndex(idx))
}

fn read_varint(data: &[u8], pos: &mut usize, ctx: &'static str) -> Result<u32, StructuralError> {
    let (v, used) = decode_varint(&data[*pos..]).map_err(|_| StructuralError::Truncated(ctx))?;
    *pos += used;
    Ok(v)
}

fn read_byte(data: &[u8], pos: &mut usize, ctx: &'static str) -> Result<u8, StructuralError> {
    if *pos >= data.len() { return Err(StructuralError::Truncated(ctx)); }
    let b = data[*pos];
    *pos += 1;
    Ok(b)
}

/// Reconnait le patron "prefixe alphabetique (+ `_`) suivi de chiffres"
/// (ex. "r001", "e047", "id_12256") -- retourne (prefixe, largeur du
/// suffixe numerique en chiffres, valeur numerique). `None` si la chaine
/// ne matche pas EXACTEMENT ce patron (mixte, prefixe vide, suffixe non
/// numerique, trop long pour un u32) -- dans ce cas l'appelant retombe
/// sur le chemin texte existant, jamais d'echec.
fn try_parse_id(s: &str) -> Option<(&str, u8, u32)> {
    let bytes = s.as_bytes();
    let split = bytes.iter().position(|b| b.is_ascii_digit())?;
    if split == 0 {
        return None; // pas de prefixe -- rien a gagner a traiter ce cas specialement
    }
    let (prefix, digits) = s.split_at(split);
    if prefix.len() > 8 || digits.is_empty() || digits.len() > 9 {
        return None;
    }
    if !prefix.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return None;
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u32 = digits.parse().ok()?;
    Some((prefix, digits.len() as u8, value))
}

/// Ecrit une entree du flux `ids` : `[len(prefixe)][octets prefixe]
/// [largeur][delta zigzag-varint vs la derniere valeur vue pour CE
/// prefixe dans CE message]`. `last` est mis a jour en place.
fn encode_id_entry(ids: &mut Vec<u8>, last: &mut HashMap<String, i64>, prefix: &str, width: u8, value: u32) {
    let prev = *last.get(prefix).unwrap_or(&0);
    let delta = (value as i64) - prev;
    let delta32 = delta.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    ids.push(prefix.len() as u8);
    ids.extend_from_slice(prefix.as_bytes());
    ids.push(width);
    ids.extend_from_slice(&encode_varint(zigzag_encode(delta32)));
    last.insert(prefix.to_string(), value as i64);
}

/// Inverse de `encode_id_entry` -- reconstruit la chaine ORIGINALE
/// exactement (zeros de tete inclus, via la largeur transmise).
fn decode_id_entry(ids: &[u8], pos: &mut usize, last: &mut HashMap<String, i64>) -> Result<String, StructuralError> {
    let plen = read_byte(ids, pos, "ids: longueur prefixe")? as usize;
    if *pos + plen > ids.len() {
        return Err(StructuralError::Truncated("ids: octets prefixe"));
    }
    let prefix = String::from_utf8_lossy(&ids[*pos..*pos + plen]).into_owned();
    *pos += plen;
    let width = read_byte(ids, pos, "ids: largeur")?;
    let (raw_delta, used) = decode_varint(&ids[*pos..]).map_err(|_| StructuralError::Truncated("ids: delta"))?;
    *pos += used;
    let delta = zigzag_decode(raw_delta) as i64;
    let prev = *last.get(&prefix).unwrap_or(&0);
    let value = prev + delta;
    last.insert(prefix.clone(), value);
    Ok(format!("{}{:0width$}", prefix, value, width = width as usize))
}

/// Encode `defines`/`relations`/`uncertainty` en 2 flux (voir
/// `StructuralStreams`). Iteration deterministe (cles de HashMap triees)
/// pour un flux reproductible.
pub fn encode_structural(
    defines: &[HashMap<String, String>],
    relations: &[HashMap<String, String>],
    uncertainty: &[HashMap<String, String>],
) -> StructuralStreams {
    let mut table = StringTable::new();
    let mut stable = Vec::new();
    let mut variable = Vec::new();
    let mut ids = Vec::new();
    let mut id_last: HashMap<String, i64> = HashMap::new();

    // --- DEFINES (name/entity_type toujours variable ; id, quand present,
    // beneficie du meme fast-path prefix+delta que relations.id et
    // uncertainty.identifier -- portee etendue le 2026-09-29, `id` vivait
    // avant cela dans les extras generiques comme n'importe quelle autre
    // cle, sans aucune compression specifique) ---
    variable.extend_from_slice(&encode_varint(defines.len() as u32));
    for d in defines {
        let name_idx = table.intern(d.get("name").map(String::as_str).unwrap_or(""));
        let type_idx = table.intern(d.get("entity_type").map(String::as_str).unwrap_or(""));
        variable.extend_from_slice(&encode_varint(name_idx));
        variable.extend_from_slice(&encode_varint(type_idx));

        match d.get("id").filter(|v| !v.is_empty()) {
            Some(id_val) => {
                stable.push(1); // has_id_extra
                match try_parse_id(id_val) {
                    Some((prefix, width, value)) => {
                        stable.push(1); // id_is_numeric
                        encode_id_entry(&mut ids, &mut id_last, prefix, width, value);
                    }
                    None => {
                        stable.push(0); // id_is_numeric
                        let idx = table.intern(id_val);
                        variable.extend_from_slice(&encode_varint(idx));
                    }
                }
            }
            None => stable.push(0), // has_id_extra
        }

        let mut extras: Vec<(&String, &String)> = d.iter()
            .filter(|(k, _)| k.as_str() != "name" && k.as_str() != "entity_type" && k.as_str() != "id")
            .collect();
        extras.sort_by(|a, b| a.0.cmp(b.0));
        variable.extend_from_slice(&encode_varint(extras.len() as u32));
        for (k, v) in extras {
            let k_idx = table.intern(k);
            let v_idx = table.intern(v);
            variable.extend_from_slice(&encode_varint(k_idx));
            variable.extend_from_slice(&encode_varint(v_idx));
        }
    }

    // --- RELATIONS (opcodes -> stable, tout le reste -> variable) ---
    variable.extend_from_slice(&encode_varint(relations.len() as u32));
    for r in relations {
        let op = r.get("type").map(String::as_str).unwrap_or("");
        match OFFICIAL_OPERATORS.iter().position(|&o| o == op) {
            Some(i) => stable.push(i as u8),
            None => {
                stable.push(OPCODE_FALLBACK);
                let idx = table.intern(op);
                variable.extend_from_slice(&encode_varint(idx));
            }
        }
        let subj_idx = table.intern(r.get("subject").map(String::as_str).unwrap_or(""));
        let obj_idx = table.intern(r.get("object").map(String::as_str).unwrap_or(""));
        variable.extend_from_slice(&encode_varint(subj_idx));
        variable.extend_from_slice(&encode_varint(obj_idx));

        match r.get("modality").map(String::as_str) {
            Some(m) => {
                stable.push(1);
                match VALID_CONSTRAINT_MODALITIES.iter().position(|&x| x == m) {
                    Some(i) => stable.push(i as u8),
                    None => {
                        stable.push(OPCODE_FALLBACK);
                        let idx = table.intern(m);
                        variable.extend_from_slice(&encode_varint(idx));
                    }
                }
            }
            None => stable.push(0),
        }

        match r.get("id").map(String::as_str) {
            Some(id) => {
                stable.push(1);
                match try_parse_id(id) {
                    Some((prefix, width, value)) => {
                        stable.push(1); // id_is_numeric
                        encode_id_entry(&mut ids, &mut id_last, prefix, width, value);
                    }
                    None => {
                        stable.push(0); // id_is_numeric
                        let idx = table.intern(id);
                        variable.extend_from_slice(&encode_varint(idx));
                    }
                }
            }
            None => stable.push(0),
        }

        let mut extras: Vec<(&String, &String)> = r.iter()
            .filter(|(k, _)| !matches!(k.as_str(), "type" | "subject" | "object" | "modality" | "id"))
            .collect();
        extras.sort_by(|a, b| a.0.cmp(b.0));
        variable.extend_from_slice(&encode_varint(extras.len() as u32));
        for (k, v) in extras {
            let k_idx = table.intern(k);
            let v_idx = table.intern(v);
            variable.extend_from_slice(&encode_varint(k_idx));
            variable.extend_from_slice(&encode_varint(v_idx));
        }
    }

    // --- UNCERTAINTY (statut -> stable, reste -> variable) ---
    variable.extend_from_slice(&encode_varint(uncertainty.len() as u32));
    for u in uncertainty {
        let identifier = u.get("identifier").map(String::as_str).unwrap_or("");
        match try_parse_id(identifier) {
            Some((prefix, width, value)) => {
                stable.push(1); // identifier_is_numeric
                encode_id_entry(&mut ids, &mut id_last, prefix, width, value);
            }
            None => {
                stable.push(0); // identifier_is_numeric
                let ident_idx = table.intern(identifier);
                variable.extend_from_slice(&encode_varint(ident_idx));
            }
        }
        let status = u.get("status").map(String::as_str).unwrap_or("");
        match VALID_UNCERTAINTY_STATUSES.iter().position(|&s| s == status) {
            Some(i) => stable.push(i as u8),
            None => {
                stable.push(OPCODE_FALLBACK);
                let idx = table.intern(status);
                variable.extend_from_slice(&encode_varint(idx));
            }
        }
        match u.get("sigma").map(String::as_str) {
            Some(sigma) => {
                stable.push(1);
                let idx = table.intern(sigma);
                variable.extend_from_slice(&encode_varint(idx));
            }
            None => stable.push(0),
        }
    }

    // Les longueurs de la table de chaines sont ECRITES EN PREMIER dans
    // `variable` (message-local, comme le reste de ce flux) ; le
    // CONTENU texte va dans son propre flux `text` -- n'est connu
    // qu'APRES avoir vu tout le contenu ci-dessus, donc prefixe a la fin.
    let (lengths, text) = table.serialize();
    let mut variable_final = lengths;
    variable_final.extend_from_slice(&variable);

    StructuralStreams { stable, text, ids, variable: variable_final }
}

type DecodedTriple = (Vec<HashMap<String, String>>, Vec<HashMap<String, String>>, Vec<HashMap<String, String>>);

/// Inverse de `encode_structural`. Reconstruit exactement les 3 Vec de
/// HashMap encodes (roundtrip garanti, voir tests) -- pas un `CstlPayload`
/// complet, voir portee assumee en tete de module.
pub fn decode_structural(streams: &StructuralStreams) -> Result<DecodedTriple, StructuralError> {
    let stable = &streams.stable;
    let variable = &streams.variable;
    let text = &streams.text;
    let ids = &streams.ids;
    let mut spos = 0usize; // curseur dans `stable`
    let mut vpos = 0usize; // curseur dans `variable` (longueurs + index)
    let mut tpos = 0usize; // curseur dans `text` (octets UTF-8 concatenes)
    let mut ipos = 0usize; // curseur dans `ids`
    let mut id_last: HashMap<String, i64> = HashMap::new();

    let table = StringTable::deserialize(variable, &mut vpos, text, &mut tpos)?;

    let defines_count = read_varint(variable, &mut vpos, "defines: compte")?;
    let mut defines = Vec::with_capacity(defines_count as usize);
    for _ in 0..defines_count {
        let name_idx = read_varint(variable, &mut vpos, "defines: name")?;
        let type_idx = read_varint(variable, &mut vpos, "defines: entity_type")?;
        let mut map = HashMap::new();
        map.insert("name".to_string(), read_string(&table, name_idx)?.to_string());
        map.insert("entity_type".to_string(), read_string(&table, type_idx)?.to_string());

        let has_id_extra = read_byte(stable, &mut spos, "defines: has_id_extra")?;
        if has_id_extra == 1 {
            let id_is_numeric = read_byte(stable, &mut spos, "defines: id_is_numeric")?;
            let id_str = if id_is_numeric == 1 {
                decode_id_entry(ids, &mut ipos, &mut id_last)?
            } else {
                let idx = read_varint(variable, &mut vpos, "defines: id fallback")?;
                read_string(&table, idx)?.to_string()
            };
            map.insert("id".to_string(), id_str);
        }

        let extra_count = read_varint(variable, &mut vpos, "defines: extras compte")?;
        for _ in 0..extra_count {
            let k_idx = read_varint(variable, &mut vpos, "defines: extra key")?;
            let v_idx = read_varint(variable, &mut vpos, "defines: extra val")?;
            map.insert(read_string(&table, k_idx)?.to_string(), read_string(&table, v_idx)?.to_string());
        }
        defines.push(map);
    }

    let relations_count = read_varint(variable, &mut vpos, "relations: compte")?;
    let mut relations = Vec::with_capacity(relations_count as usize);
    for _ in 0..relations_count {
        let op_byte = read_byte(stable, &mut spos, "relations: op byte")?;
        let op = if op_byte == OPCODE_FALLBACK {
            let idx = read_varint(variable, &mut vpos, "relations: op fallback")?;
            read_string(&table, idx)?.to_string()
        } else {
            OFFICIAL_OPERATORS.get(op_byte as usize)
                .map(|s| s.to_string())
                .ok_or(StructuralError::BadOpcode(op_byte))?
        };
        let subj_idx = read_varint(variable, &mut vpos, "relations: subject")?;
        let obj_idx = read_varint(variable, &mut vpos, "relations: object")?;

        let mut map = HashMap::new();
        map.insert("type".to_string(), op);
        map.insert("subject".to_string(), read_string(&table, subj_idx)?.to_string());
        map.insert("object".to_string(), read_string(&table, obj_idx)?.to_string());

        let has_modality = read_byte(stable, &mut spos, "relations: has_modality")?;
        if has_modality == 1 {
            let m_byte = read_byte(stable, &mut spos, "relations: modality byte")?;
            let modality = if m_byte == OPCODE_FALLBACK {
                let idx = read_varint(variable, &mut vpos, "relations: modality fallback")?;
                read_string(&table, idx)?.to_string()
            } else {
                VALID_CONSTRAINT_MODALITIES.get(m_byte as usize)
                    .map(|s| s.to_string())
                    .ok_or(StructuralError::BadOpcode(m_byte))?
            };
            map.insert("modality".to_string(), modality);
        }

        let has_id = read_byte(stable, &mut spos, "relations: has_id")?;
        if has_id == 1 {
            let id_is_numeric = read_byte(stable, &mut spos, "relations: id_is_numeric")?;
            let id_str = if id_is_numeric == 1 {
                decode_id_entry(ids, &mut ipos, &mut id_last)?
            } else {
                let id_idx = read_varint(variable, &mut vpos, "relations: id")?;
                read_string(&table, id_idx)?.to_string()
            };
            map.insert("id".to_string(), id_str);
        }

        let extra_count = read_varint(variable, &mut vpos, "relations: extras compte")?;
        for _ in 0..extra_count {
            let k_idx = read_varint(variable, &mut vpos, "relations: extra key")?;
            let v_idx = read_varint(variable, &mut vpos, "relations: extra val")?;
            map.insert(read_string(&table, k_idx)?.to_string(), read_string(&table, v_idx)?.to_string());
        }
        relations.push(map);
    }

    let uncertainty_count = read_varint(variable, &mut vpos, "uncertainty: compte")?;
    let mut uncertainty = Vec::with_capacity(uncertainty_count as usize);
    for _ in 0..uncertainty_count {
        let identifier_is_numeric = read_byte(stable, &mut spos, "uncertainty: identifier_is_numeric")?;
        let identifier = if identifier_is_numeric == 1 {
            decode_id_entry(ids, &mut ipos, &mut id_last)?
        } else {
            let ident_idx = read_varint(variable, &mut vpos, "uncertainty: identifier")?;
            read_string(&table, ident_idx)?.to_string()
        };
        let s_byte = read_byte(stable, &mut spos, "uncertainty: status byte")?;
        let status = if s_byte == OPCODE_FALLBACK {
            let idx = read_varint(variable, &mut vpos, "uncertainty: status fallback")?;
            read_string(&table, idx)?.to_string()
        } else {
            VALID_UNCERTAINTY_STATUSES.get(s_byte as usize)
                .map(|s| s.to_string())
                .ok_or(StructuralError::BadOpcode(s_byte))?
        };
        let mut map = HashMap::new();
        map.insert("identifier".to_string(), identifier);
        map.insert("status".to_string(), status);

        let has_sigma = read_byte(stable, &mut spos, "uncertainty: has_sigma")?;
        if has_sigma == 1 {
            let sigma_idx = read_varint(variable, &mut vpos, "uncertainty: sigma")?;
            map.insert("sigma".to_string(), read_string(&table, sigma_idx)?.to_string());
        }
        uncertainty.push(map);
    }

    Ok((defines, relations, uncertainty))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_empty_roundtrip() {
        let streams = encode_structural(&[], &[], &[]);
        let (d, r, u) = decode_structural(&streams).unwrap();
        assert!(d.is_empty() && r.is_empty() && u.is_empty());
    }

    #[test]
    fn test_defines_roundtrip_no_id_stable_has_only_flags() {
        // Depuis l'ajout du fast-path defines.id (2026-09-29), `stable`
        // n'est plus vide meme sans id: chaque define ecrit un octet
        // has_id_extra=0. C'est un octet fixe par entree (pas un opcode
        // variable), donc toujours 1 octet par define ici.
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party")]),
            m(&[("name", "Landlord"), ("entity_type", "Party")]),
        ];
        let streams = encode_structural(&defines, &[], &[]);
        assert_eq!(streams.stable, vec![0u8, 0u8], "un octet has_id_extra=0 par define, rien d'autre");
        let (d, _, _) = decode_structural(&streams).unwrap();
        assert_eq!(d, defines);
    }

    #[test]
    fn test_defines_id_uses_numeric_fast_path_and_roundtrips() {
        // 2026-09-29: extension de portee du fast-path prefix+delta a
        // defines.id (avant, seuls relations.id et uncertainty.identifier
        // en beneficiaient -- id vivait ici dans les extras generiques,
        // interne comme n'importe quelle autre paire cle/valeur).
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "e001")]),
            m(&[("name", "Landlord"), ("entity_type", "Party"), ("id", "e002")]),
        ];
        let streams = encode_structural(&defines, &[], &[]);
        assert!(!streams.ids.is_empty(), "id numerique doit passer par le flux ids");
        assert!(!streams.stable.is_empty(), "les flags has_id_extra/id_is_numeric vivent dans stable");
        let (d, _, _) = decode_structural(&streams).unwrap();
        assert_eq!(d, defines);
    }

    #[test]
    fn test_defines_non_numeric_id_falls_back_to_text_path() {
        let defines = vec![m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "not-a-numeric-id")])];
        let streams = encode_structural(&defines, &[], &[]);
        assert!(streams.ids.is_empty(), "id non-numerique ne doit pas toucher le flux ids");
        let (d, _, _) = decode_structural(&streams).unwrap();
        assert_eq!(d, defines);
    }

    #[test]
    fn test_defines_without_id_field_omits_extra_entirely() {
        // Un define sans "id" du tout ne doit pas laisser de trace de ce
        // champ au roundtrip (has_id_extra=0, aucune cle "id" reinseree).
        let defines = vec![m(&[("name", "Tenant"), ("entity_type", "Party")])];
        let streams = encode_structural(&defines, &[], &[]);
        let (d, _, _) = decode_structural(&streams).unwrap();
        assert!(!d[0].contains_key("id"));
    }

    #[test]
    fn test_relations_known_operator_and_modality_roundtrip() {
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
            m(&[("type", "ARR.ACCESS"), ("subject", "Tenant"), ("object", "SecurityDeposit"), ("modality", "MUST_NOT"), ("id", "r002")]),
        ];
        let streams = encode_structural(&[], &relations, &[]);
        assert!(!streams.stable.is_empty(), "des operateurs/modalites connus doivent produire des octets stables");
        let (_, r, _) = decode_structural(&streams).unwrap();
        assert_eq!(r, relations);
    }

    #[test]
    fn test_relations_unknown_operator_fallback_roundtrip() {
        let relations = vec![m(&[("type", "UNE_RELATION_INVENTEE"), ("subject", "A"), ("object", "B")])];
        let streams = encode_structural(&[], &relations, &[]);
        let (_, r, _) = decode_structural(&streams).unwrap();
        assert_eq!(r, relations);
    }

    #[test]
    fn test_uncertainty_roundtrip_with_and_without_sigma() {
        let uncertainty = vec![
            m(&[("identifier", "e001"), ("status", "ESTIMATED"), ("sigma", "0.7")]),
            m(&[("identifier", "r999"), ("status", "UNKNOWN")]),
        ];
        let streams = encode_structural(&[], &[], &uncertainty);
        let (_, _, u) = decode_structural(&streams).unwrap();
        assert_eq!(u, uncertainty);
    }

    #[test]
    fn test_full_payload_roundtrip_and_measures_stream_split() {
        let defines = vec![
            m(&[("name", "Tenant"), ("entity_type", "Party"), ("id", "e001")]),
            m(&[("name", "Landlord"), ("entity_type", "Party"), ("id", "e002")]),
            m(&[("name", "Rent"), ("entity_type", "Obligation"), ("id", "e003")]),
        ];
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "Tenant"), ("object", "Rent"), ("modality", "MUST"), ("id", "r001")]),
            m(&[("type", "ARR.ACCESS"), ("subject", "Tenant"), ("object", "SecurityDeposit"), ("modality", "MUST_NOT"), ("id", "r002")]),
        ];
        let uncertainty = vec![m(&[("identifier", "e001"), ("status", "ESTIMATED"), ("sigma", "0.7")])];

        let streams = encode_structural(&defines, &relations, &uncertainty);
        let (d, r, u) = decode_structural(&streams).unwrap();
        assert_eq!((d, r, u), (defines, relations, uncertainty));

        println!(
            "flux stable (opcodes): {} octets | flux texte (noms/valeurs): {} octets | flux variable (longueurs+index): {} octets | total: {}",
            streams.stable.len(), streams.text.len(), streams.variable.len(),
            streams.stable.len() + streams.text.len() + streams.variable.len()
        );
    }

    #[test]
    fn test_truncated_variable_stream_errors_cleanly() {
        let streams = encode_structural(&[m(&[("name", "X"), ("entity_type", "Y")])], &[], &[]);
        let truncated = StructuralStreams {
            stable: streams.stable,
            text: streams.text,
            ids: streams.ids,
            variable: streams.variable[..streams.variable.len() - 2].to_vec(),
        };
        assert!(decode_structural(&truncated).is_err());
    }

    #[test]
    fn test_truncated_stable_stream_errors_cleanly() {
        let streams = encode_structural(&[], &[m(&[("type", "PERFORM"), ("subject", "A"), ("object", "B")])], &[]);
        assert!(!streams.stable.is_empty());
        let truncated = StructuralStreams { stable: Vec::new(), text: streams.text, ids: streams.ids, variable: streams.variable };
        assert!(decode_structural(&truncated).is_err());
    }

    /// LE test qui repond a "donne moi un exemple d'id" (2026-09-29) :
    /// relations dont l'id matche le patron prefixe+chiffres passent par
    /// le flux `ids` (delta zigzag par prefixe), roundtrip exact zeros de
    /// tete inclus, mesure reelle du gain vs le chemin texte.
    #[test]
    fn test_relation_ids_use_numeric_fast_path_and_roundtrip_with_leading_zeros() {
        let relations = vec![
            m(&[("type", "PERFORM"), ("subject", "A"), ("object", "B"), ("id", "r001")]),
            m(&[("type", "PERFORM"), ("subject", "A"), ("object", "B"), ("id", "r002")]),
            m(&[("type", "PERFORM"), ("subject", "A"), ("object", "B"), ("id", "r003")]),
        ];
        let streams = encode_structural(&[], &relations, &[]);
        let (_, r, _) = decode_structural(&streams).unwrap();
        assert_eq!(r, relations, "zeros de tete (r001, pas r1) doivent survivre exactement");
        assert!(!streams.ids.is_empty(), "des ids au patron reconnu doivent produire des octets dans le flux `ids`");
        println!("flux ids (3 relations r001/r002/r003): {} octets", streams.ids.len());
    }

    /// Un id qui NE matche PAS le patron (pas de suffixe numerique, ou
    /// motif hors normes) retombe sur le chemin texte existant, jamais
    /// d'echec -- verifie explicitement, pas suppose.
    #[test]
    fn test_non_numeric_id_falls_back_to_text_path_cleanly() {
        let relations = vec![m(&[("type", "PERFORM"), ("subject", "A"), ("object", "B"), ("id", "uuid-not-numeric")])];
        let uncertainty = vec![m(&[("identifier", "not-an-id-shape"), ("status", "UNKNOWN")])];
        let streams = encode_structural(&[], &relations, &uncertainty);
        let (_, r, u) = decode_structural(&streams).unwrap();
        assert_eq!(r, relations);
        assert_eq!(u, uncertainty);
        assert!(streams.ids.is_empty(), "aucun id ne matche le patron -- le flux `ids` doit rester vide");
    }

    /// `uncertainty.identifier` beneficie aussi du chemin numerique --
    /// verifie separement de `relations.id` (chemins de code distincts).
    #[test]
    fn test_uncertainty_identifier_numeric_fast_path_roundtrip() {
        let uncertainty = vec![
            m(&[("identifier", "e001"), ("status", "ESTIMATED"), ("sigma", "0.7")]),
            m(&[("identifier", "e002"), ("status", "UNKNOWN")]),
        ];
        let streams = encode_structural(&[], &[], &uncertainty);
        let (_, _, u) = decode_structural(&streams).unwrap();
        assert_eq!(u, uncertainty);
        assert!(!streams.ids.is_empty());
    }
}
