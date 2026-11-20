//! Integrite de l'historique d'audit persiste (2026-11-20).
//!
//! Constate en direct avant ce correctif : un message reecrit dans la base
//! SQLite ("shipped" -> "refunded"), ou un maillon supprime, etaient
//! recharges au redemarrage SANS AUCUNE alerte, et le graphe affichait le
//! contenu falsifie. Ces tests figent la detection et le refus de demarrage.

use cstl_parser::adn_store::AdnStore;
use cstl_parser::server::audit::HashChain;
use cstl_parser::server::parser::parse_payload;
use cstl_parser::server::CstlNativeServer;
use rusqlite::Connection;

fn temp_db(tag: &str) -> String {
    let p = std::env::temp_dir().join(format!(
        "cstl_integrity_{tag}_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    p.to_str().unwrap().to_string()
}

/// Ecrit 3 messages comme le fait le serveur : chaine + message + relations.
fn seed(path: &str) -> Vec<String> {
    let store = AdnStore::open(path).unwrap();
    let mut chain = HashChain::new();
    let mut hashes = Vec::new();
    for object in ["approved", "shipped", "closed"] {
        let raw = format!(
            "#!CSTL v5.0.0 MODE=A\nMETA [encoder=t, produced_by=t]\n\
             INTENT_PAYLOAD [purpose=inform, sender=alice, receiver=bob]\n\
             RELATION [type=STATE, subject=payment_{object}, object={object}]\n---END---\n"
        );
        let payload = parse_payload(&raw).unwrap();
        let entry = chain.append(&payload);
        store.save_audit_entry(&entry).unwrap();
        store.put(&entry.hash, &raw, Some("t"), Some("t"), 0.5, Some(&entry.parent_hash), None, None).unwrap();
        store.put_relations(&entry.hash, &payload.relations).unwrap();
        hashes.push(entry.hash);
    }
    hashes
}

fn problems(path: &str) -> Vec<String> {
    let store = AdnStore::open(path).unwrap();
    let chain = store.load_chain().unwrap();
    store.verify_integrity(&chain).unwrap().problems
}

#[test]
fn clean_history_verifies_and_server_starts() {
    let path = temp_db("clean");
    seed(&path);
    let store = AdnStore::open(&path).unwrap();
    let report = store.verify_integrity(&store.load_chain().unwrap()).unwrap();
    assert_eq!((report.entries, report.content_verified, report.problems.len()), (3, 3, 0), "{report:?}");
    drop(store);
    assert!(CstlNativeServer::try_with_data_path(0, &path).is_ok());
    std::fs::remove_file(&path).ok();
}

#[test]
fn rewritten_message_content_is_detected_and_startup_refused() {
    let path = temp_db("content");
    let hashes = seed(&path);
    let c = Connection::open(&path).unwrap();
    let payload: Vec<u8> = c.query_row("SELECT payload FROM adn_store WHERE hash=?1", [&hashes[1]], |r| r.get(0)).unwrap();
    let forged = String::from_utf8(payload).unwrap().replace("object=shipped", "object=refunded");
    c.execute("UPDATE adn_store SET payload=?1 WHERE hash=?2", rusqlite::params![forged.as_bytes(), &hashes[1]]).unwrap();
    drop(c);
    let p = problems(&path);
    assert!(p.iter().any(|m| m.contains("seq=1") && m.contains("contenu modifie")), "{p:?}");
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("historique d'audit compromis"), "{err}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn deleted_link_is_detected() {
    let path = temp_db("link");
    seed(&path);
    Connection::open(&path).unwrap().execute("DELETE FROM audit_trail WHERE seq=1", []).unwrap();
    let p = problems(&path);
    assert!(p.iter().any(|m| m.contains("lien rompu")), "{p:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn tampered_derived_relations_are_detected() {
    let path = temp_db("rels");
    seed(&path);
    Connection::open(&path).unwrap()
        .execute("UPDATE adn_relations SET object='refunded' WHERE object='shipped'", []).unwrap();
    let p = problems(&path);
    assert!(p.iter().any(|m| m.contains("relations derivees")), "{p:?}");
    std::fs::remove_file(&path).ok();
}
