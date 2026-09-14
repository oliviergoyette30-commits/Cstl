//! Test example for REST API server integration
//! This tests compilation of the REST API module independently

use std::sync::Arc;
use tokio::sync::Mutex;
use cstl_parser::adn_store::AdnStore;
use cstl_parser::server::rest_api;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create in-memory database
    eprintln!("Creating in-memory ADN store...");
    let adn_store = AdnStore::open(":memory:")?;
    let adn_store = Arc::new(Mutex::new(adn_store));

    // Test that router can be created
    eprintln!("Creating REST API router...");
    let _router = rest_api::create_router(adn_store.clone());

    eprintln!("✅ REST API compilation test passed");
    Ok(())
}
