//! CSTL v4.9.3 — cstl_validate CLI
//!
//! Reads a CSTL payload from a file argument or stdin, runs the full
//! parse + validate pipeline, prints a report, and exits non-zero if invalid.
//!
//! Usage:
//!   cstl_validate <file.cstl>
//!   cat payload.cstl | cstl_validate
//!
//! Exit codes: 0 = valid, 1 = invalid, 2 = I/O error.

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

use cstl_parser::parse;

fn main() {
    let args: Vec<String> = env::args().collect();

    let input = if args.len() > 1 {
        match fs::read_to_string(&args[1]) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("error: cannot read '{}': {}", args[1], e);
                process::exit(2);
            }
        }
    } else {
        let mut buf = String::new();
        if io::stdin().read_to_string(&mut buf).is_err() {
            eprintln!("error: cannot read stdin");
            process::exit(2);
        }
        buf
    };

    let doc = parse(&input);

    println!("CSTL validate — v4.9.3");
    println!("  valid : {}", doc.is_valid);

    if !doc.errors.is_empty() {
        println!("  errors:");
        for e in &doc.errors {
            println!("    - {}", e);
        }
    }
    if !doc.warnings.is_empty() {
        println!("  warnings:");
        for w in &doc.warnings {
            println!("    - {}", w);
        }
    }

    process::exit(if doc.is_valid { 0 } else { 1 });
}
