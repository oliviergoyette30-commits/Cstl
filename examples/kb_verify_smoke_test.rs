use cstl_parser::kb_verify::KbVerifier;

#[tokio::main]
async fn main() {
    let verifier = KbVerifier::new();
    let result = verifier.verify_relation("Marie Curie", "born_in", "Warsaw", "fr", 4, 40).await;
    println!("{:#?}", result);
}
