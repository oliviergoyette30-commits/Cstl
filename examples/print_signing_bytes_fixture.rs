/// examples/print_signing_bytes_fixture.rs -- imprime signing_bytes() en hex
/// pour un payload FIXE, afin de le comparer octet par octet au port Python
/// (sdk/python/cstl_llm_agent.py::cstl_signing_bytes). Seule partie de la
/// feature C reellement verifiable dans ce sandbox (pas de paquet
/// `anthropic` ni de cle API disponibles ici) -- toute derive entre les
/// deux implementations casserait silencieusement toutes les signatures
/// produites cote Python.
use cstl_parser::server::audit::signing_bytes;
use cstl_parser::server::parser::parse_payload;

fn main() {
    // Inclut deliberement: un accent (café, teste la normalisation NFC),
    // une valeur avec virgule/quotee (teste le parsing top-level), une
    // RELATION, et PARENT_HASH + signature + rotation_signature (doivent
    // etre EXCLUS du hash -- signature ET rotation_signature separement,
    // voir audit.rs::signing_bytes).
    let raw = "#!CSTL v5.0.0 MODE=A\n\
        META [encoder=LlmAgent, produced_by=café_agent, public_key=aabbccdd, PARENT_HASH=sha256:doitetreexclu]\n\
        INTENT_PAYLOAD [purpose=cross_lang_fixture, sender=llm_agent, receiver=server, note=\"a, b\", signature=doitetreexclu, rotation_signature=doitaussietreexclu]\n\
        RELATION [type=born_in, subject=café_test, object=Montréal]\n\
        ---END---\n";
    let payload = parse_payload(raw).expect("fixture doit parser");
    let bytes = signing_bytes(&payload);
    println!("{}", hex::encode(&bytes));
}
