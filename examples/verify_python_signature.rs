/// examples/verify_python_signature.rs -- preuve d'interoperabilite bout en
/// bout: une paire de cles Ed25519 generee EN PYTHON (cryptography,
/// sdk/python/cstl_llm_agent.py::load_or_create_keypair) signe un payload
/// via sign_intent(), et ce binaire verifie cote Rust
/// (signing::check_signature, le meme code que handler.rs utilise en
/// production) que cette signature est acceptee. Ne teste pas seulement
/// l'egalite des octets de signing_bytes() (voir
/// print_signing_bytes_fixture.rs) mais la verification cryptographique
/// reelle de bout en bout.
///
/// public_key/signature ci-dessous proviennent d'une execution reelle de
/// sign_intent() dans cette session (2026-09-06) -- pas de valeurs
/// inventees.
use cstl_parser::server::parser::parse_payload;
use cstl_parser::signing::{check_signature, SignatureCheck};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (public_key, signature) = if args.len() == 3 {
        (args[1].clone(), args[2].clone())
    } else {
        eprintln!("Usage: verify_python_signature <public_key_hex> <signature_hex>");
        std::process::exit(2);
    };

    let raw = format!(
        "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=LlmAgent, produced_by=python_interop_agent, public_key={pk}]\n\
        INTENT_PAYLOAD [purpose=interop_test, sender=python_agent, receiver=server, signature={sig}]\n\
        RELATION [type=verified_by, subject=python, object=rust]\n\
        ---END---\n",
        pk = public_key,
        sig = signature,
    );

    let payload = parse_payload(&raw).expect("payload doit parser");
    match check_signature(&payload) {
        SignatureCheck::Valid => {
            println!("VALID -- la signature Ed25519 generee par Python (cryptography) est acceptee par signing::check_signature() cote Rust.");
        }
        other => {
            println!("REJETEE: {:?}", other);
            std::process::exit(1);
        }
    }
}
