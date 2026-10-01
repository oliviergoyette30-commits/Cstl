//! src/bin/cstl_compress_cli.rs — petit outil CLI qui expose le Master
//! Compresseur (src/compression/master.rs) a des processus NON-Rust (le SDK
//! Python, en particulier). 2026-10-01, pour completer le cote EMISSION du
//! bloc wire-protocol COMPRESSED_PAYLOAD (le cote RECEPTION vit deja dans
//! server/parser.rs::record_compressed_payload).
//!
//! Pourquoi un sous-processus plutot qu'un port Python de master_compress:
//! le format de compression (4 flux, 2 dictionnaires pre-entraines,
//! delta-zigzag-varint) est deja non-trivial et teste exhaustivement cote
//! Rust (voir structural.rs, stable_dictionary.rs, text_dictionary.rs,
//! variable_delta.rs). Un deuxieme port independant en Python serait une
//! deuxieme surface a faire derapper silencieusement de l'original -- le
//! meme risque documente dans le plan de signing.rs pour cstl_signing_bytes,
//! sauf que la ici il n'y a AUCUNE raison de payer ce risque: contrairement
//! a une signature (qui doit etre calculee cote client, avant tout envoi),
//! la compression peut parfaitement vivre dans un sous-processus appele par
//! le client. Zero logique dupliquee, zero drift possible.
//!
//! Usage:
//!   cstl_compress_cli compress   < triple.json   > data.b64
//!   cstl_compress_cli decompress < data.b64      > triple.json
//!
//! Format JSON du triple (stdin pour compress, stdout pour decompress):
//!   {"defines": [{"name": "x", ...}, ...], "relations": [...], "uncertainty": [...]}
//! -- liste de HashMap<String,String>, un objet JSON plat par entree.
//!
//! Deux modes additionnels (2026-10-01), pour le cote COMPRESSED_RESPONSE
//! (compression des REPONSES du serveur, voir
//! src/server/response_compression.rs et src/compression/response.rs) --
//! meme raisonnement sous-processus que compress/decompress ci-dessus, mais
//! sur des blocs CSTL GENERIQUES (pas le schema defines/relations/
//! uncertainty), donc un format JSON different (liste ordonnee de blocs,
//! chaque bloc = [nom, [[cle,valeur], ...]] -- des tuples, pas un objet, pour
//! preserver l'ORDRE des champs et ne jamais perdre une cle dupliquee):
//!
//!   cstl_compress_cli compress-response   < blocks.json  > data.b64
//!   cstl_compress_cli decompress-response < data.b64      > blocks.json
//!
//! Format JSON (stdin pour compress-response, stdout pour
//! decompress-response):
//!   [["AUDIT", [["hash", "sha256:..."], ["seq", "0"]]], ["META", [...]]]

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use base64::Engine as _;
use cstl_parser::compression::master::{master_compress, master_decompress};
use cstl_parser::compression::response::{compress_response_blocks, decompress_response_blocks, ResponseBlock};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
struct Triple {
    #[serde(default)]
    defines: Vec<HashMap<String, String>>,
    #[serde(default)]
    relations: Vec<HashMap<String, String>>,
    #[serde(default)]
    uncertainty: Vec<HashMap<String, String>>,
}

fn read_stdin_all() -> io::Result<String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn run_compress() -> Result<(), String> {
    let raw = read_stdin_all().map_err(|e| format!("lecture stdin: {e}"))?;
    let triple: Triple = serde_json::from_str(raw.trim())
        .map_err(|e| format!("JSON d'entree invalide (attendu {{defines,relations,uncertainty}}): {e}"))?;

    let compressed = master_compress(&triple.defines, &triple.relations, &triple.uncertainty)
        .map_err(|e| format!("master_compress: {e}"))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    print!("{b64}");
    io::stdout().flush().map_err(|e| format!("ecriture stdout: {e}"))?;
    Ok(())
}

fn run_decompress() -> Result<(), String> {
    let raw = read_stdin_all().map_err(|e| format!("lecture stdin: {e}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("base64 d'entree invalide: {e}"))?;

    let (defines, relations, uncertainty) =
        master_decompress(&bytes).map_err(|e| format!("master_decompress: {e}"))?;

    let triple = Triple { defines, relations, uncertainty };
    let json = serde_json::to_string(&triple).map_err(|e| format!("serialisation JSON: {e}"))?;
    print!("{json}");
    io::stdout().flush().map_err(|e| format!("ecriture stdout: {e}"))?;
    Ok(())
}

fn run_compress_response() -> Result<(), String> {
    let raw = read_stdin_all().map_err(|e| format!("lecture stdin: {e}"))?;
    let blocks: Vec<ResponseBlock> = serde_json::from_str(raw.trim())
        .map_err(|e| format!("JSON d'entree invalide (attendu [[nom, [[cle,valeur],...]], ...]): {e}"))?;

    let compressed = compress_response_blocks(&blocks);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    print!("{b64}");
    io::stdout().flush().map_err(|e| format!("ecriture stdout: {e}"))?;
    Ok(())
}

fn run_decompress_response() -> Result<(), String> {
    let raw = read_stdin_all().map_err(|e| format!("lecture stdin: {e}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| format!("base64 d'entree invalide: {e}"))?;

    let blocks = decompress_response_blocks(&bytes).map_err(|e| format!("decompress_response_blocks: {e}"))?;

    let json = serde_json::to_string(&blocks).map_err(|e| format!("serialisation JSON: {e}"))?;
    print!("{json}");
    io::stdout().flush().map_err(|e| format!("ecriture stdout: {e}"))?;
    Ok(())
}

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    let result = match mode.as_deref() {
        Some("compress") => run_compress(),
        Some("decompress") => run_decompress(),
        Some("compress-response") => run_compress_response(),
        Some("decompress-response") => run_decompress_response(),
        _ => {
            eprintln!("usage: cstl_compress_cli <compress|decompress|compress-response|decompress-response>");
            eprintln!("  compress:            stdin = JSON {{defines,relations,uncertainty}} -> stdout = base64");
            eprintln!("  decompress:          stdin = base64                                -> stdout = JSON triple");
            eprintln!("  compress-response:   stdin = JSON [[nom,[[k,v],...]],...]           -> stdout = base64");
            eprintln!("  decompress-response: stdin = base64                                -> stdout = JSON blocs");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cstl_compress_cli: {e}");
            ExitCode::FAILURE
        }
    }
}
