//! Test example for REST API server integration
//! This tests compilation of the REST API module independently

use std::sync::Arc;
use tokio::sync::Mutex;
use cstl_parser::adn_store::AdnStore;
use cstl_parser::server::audit::HashChain;
use cstl_parser::server::deontic_orchestration::DeonticOrchestrator;
use cstl_parser::server::rest_api;
use cstl_parser::governance::GovernanceTracker;
use cstl_parser::restricted_council::RestrictedCouncil;
use cstl_parser::server::wai::DictionaryRegistry;
use cstl_parser::calibration::SigmaCalibrator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create in-memory database
    eprintln!("Creating in-memory ADN store...");
    let adn_store = AdnStore::open(":memory:")?;
    let adn_store = Arc::new(Mutex::new(adn_store));
    let chain = Arc::new(Mutex::new(HashChain::new()));
    let deontic = Arc::new(DeonticOrchestrator::new(
        256,
        adn_store.clone(),
        Arc::new(Mutex::new(GovernanceTracker::with_defaults())),
        Arc::new(RestrictedCouncil::single_member("olivier")),
    ));
    let wai_registry = Arc::new(DictionaryRegistry::new("cstl-test".to_string()));
    let sigma_calibrator = Arc::new(Mutex::new(SigmaCalibrator::new(0.2)));

    // Test that router can be created
    eprintln!("Creating REST API router...");
    let _router = rest_api::create_router(adn_store.clone(), chain.clone(), deontic.clone(), wai_registry.clone(), sigma_calibrator.clone());

    eprintln!("✅ REST API compilation test passed");
    Ok(())
}
