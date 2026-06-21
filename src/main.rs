use std::io::Read;
use cstl_parser::parse;

fn main() {
    // Lit le payload : 1er argument = chemin de fichier, sinon stdin.
    let input = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(&path)
            .unwrap_or_else(|e| { eprintln!("Lecture {path} impossible: {e}"); std::process::exit(1); }),
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).expect("lecture stdin");
            s
        }
    };

    let doc = parse(&input);

    println!("=== CSTL parse ===");
    println!("valide        : {}", doc.is_valid);
    println!("hashbang      : {:?}", doc.hashbang);
    println!("encoder       : {:?}", doc.encoder());
    println!("produced_by   : {:?}", doc.produced_by());
    println!("blocs         : {}", doc.blocks.len());
    for b in &doc.blocks {
        println!("  - {} ({} champs)", b.name, b.fields.len());
    }
    println!("erreurs ({})  : {:?}", doc.errors.len(), doc.errors);
    println!("warnings ({}) : {:?}", doc.warnings.len(), doc.warnings);
    println!("parse_time_us : {}", doc.parse_time_us);

    std::process::exit(if doc.is_valid { 0 } else { 1 });
}
