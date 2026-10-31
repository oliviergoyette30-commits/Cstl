/// examples/build_compressed_payload_demo.rs -- genere un VRAI bloc
/// `COMPRESSED_PAYLOAD [data=<base64>]` (wire format compresse, voir
/// `src/compression/master.rs`) a partir d'un payload CSTL exemple, pour
/// montrer a un tiers (humain ou IA en relais, cf. AGENTS.md Option 0)
/// que la compression cote CLIENT fonctionne reellement -- pas seulement
/// la compression de reponse (`compress_response=true`, deja montree
/// separement).
///
/// Imprime les DEUX formes du meme payload (brute et compressee) +
/// les tailles reelles, pour qu'on puisse les comparer a l'oeil avant
/// de les envoyer a un tiers pour test.
use base64::Engine as _;
use cstl_parser::compression::master::master_compress;
use std::collections::HashMap;

fn m(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn main() {
    let defines = vec![
        m(&[("name", "montreal"), ("entity_type", "City"), ("population", "1780000")]),
        m(&[("name", "canada"), ("entity_type", "Country")]),
    ];
    let relations = vec![
        m(&[("id", "r001"), ("type", "STATE"), ("subject", "montreal"), ("object", "canada")]),
        m(&[("id", "r002"), ("type", "CO_LOCATES"), ("subject", "montreal"), ("object", "canada")]),
    ];
    let uncertainty = vec![
        m(&[("identifier", "u001"), ("status", "MEASURED"), ("target_relation", "r001"), ("sigma", "0.0")]),
    ];

    // Reconstruit le texte brut equivalent (ce que le payload "lirait" en
    // clair) juste pour l'affichage comparatif -- la vraie source de
    // verite du contenu reste defines/relations/uncertainty ci-dessus.
    let plain = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=compression_demo, produced_by=compression_demo]\n\
         INTENT_PAYLOAD [purpose=inform, sender=alice, receiver=server]\n\
         DEFINE montreal AS City [population=1780000]\n\
         DEFINE canada AS Country\n\
         RELATION [id=r001, type=STATE, subject=montreal, object=canada]\n\
         RELATION [id=r002, type=CO_LOCATES, subject=montreal, object=canada]\n\
         UNCERTAINTY [\n\
         \u{20}u001 MEASURED [target_relation=r001, sigma=0.0]\n\
         ]\n\
         ---END---\n"
    );

    let compressed_bytes = master_compress(&defines, &relations, &uncertainty).expect("compression");
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed_bytes);

    let compressed_wire = format!(
        "#!CSTL v5.0.0 MODE=A\n\
         META [encoder=compression_demo, produced_by=compression_demo]\n\
         INTENT_PAYLOAD [purpose=inform, sender=alice, receiver=server]\n\
         COMPRESSED_PAYLOAD [data={}]\n\
         ---END---\n",
        b64
    );

    println!("=== Payload en clair (ce que le serveur comprend une fois decompresse) ===");
    println!("{plain}");
    println!("Taille: {} octets\n", plain.len());

    println!("=== Meme payload, bloc COMPRESSED_PAYLOAD (ce qu'on envoie reellement sur le fil) ===");
    println!("{compressed_wire}");
    println!("Taille: {} octets ({} octets de donnees compressees brutes avant base64)",
        compressed_wire.len(), compressed_bytes.len());

    // Round-trip immediat: prouve que ce bloc n'est pas juste plus petit,
    // il reste exploitable -- c'est ce que le serveur fait en recevant ce
    // bloc (voir parser.rs::parse_compressed_payload_block).
    let (d2, r2, u2) = cstl_parser::compression::master::master_decompress(&compressed_bytes).expect("decompression");
    assert_eq!(d2, defines);
    assert_eq!(r2, relations);
    assert_eq!(u2, uncertainty);
    println!("\nRound-trip verifie: decompresser {} redonne EXACTEMENT defines/relations/uncertainty d'origine.", "ce bloc");
}
