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

// -- Signature du serveur sur la chaine (2026-11-20) -------------------------
// Limite connue jusqu'ici : un attaquant qui REECRIT un message ET recalcule
// son hash passait les trois controles (liens, contenu, relations). La
// signature du serveur sur chaque maillon ferme ce trou.

use cstl_parser::server::server_key::ServerKey;
use std::sync::Arc;

/// Comme `seed`, mais avec la cle du serveur qui signe chaque maillon (la
/// meme cle que le serveur chargera pour ce `path`).
fn seed_signed(path: &str) -> Vec<String> {
    let key = Arc::new(ServerKey::for_data_path(path).unwrap());
    let marker = key.init_audit_marker(0, None, false).unwrap();
    let mut store = AdnStore::open(path).unwrap();
    store.set_audit_signer(key, marker);
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

/// L'attaque complete : le dernier message est reecrit, son hash RECALCULE,
/// et toutes les tables mises a jour pour rester coherentes.
fn rewrite_last_message_consistently(path: &str, last_hash: &str) {
    let c = Connection::open(path).unwrap();
    // L'attaquant a un acces direct au fichier : rien ne l'oblige a
    // respecter les cles etrangeres.
    c.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
    let payload: Vec<u8> = c.query_row("SELECT payload FROM adn_store WHERE hash=?1", [last_hash], |r| r.get(0)).unwrap();
    let forged = String::from_utf8(payload).unwrap().replace("object=closed", "object=refunded");
    let new_hash = cstl_parser::server::audit::canonical_hash(&parse_payload(&forged).unwrap());
    assert_ne!(new_hash, last_hash);
    c.execute("UPDATE adn_store SET payload=?1, hash=?2 WHERE hash=?3", rusqlite::params![forged.as_bytes(), &new_hash, last_hash]).unwrap();
    c.execute("UPDATE adn_relations SET hash=?1, object='refunded' WHERE hash=?2", rusqlite::params![&new_hash, last_hash]).unwrap();
    c.execute("UPDATE audit_trail SET hash=?1 WHERE hash=?2", rusqlite::params![&new_hash, last_hash]).unwrap();
}

/// Meme sequence qu'au demarrage du serveur (`try_with_data_path`).
fn signed_problems(path: &str) -> Vec<String> {
    let key = Arc::new(ServerKey::for_data_path(path).unwrap());
    let mut store = AdnStore::open(path).unwrap();
    let chain = store.load_chain().unwrap();
    let last = chain.entries.last().map(|e| (e.seq, e.hash.clone()));
    let marker = key.init_audit_marker(chain.entries.len() as u64, last, store.has_signed_audit_entries().unwrap()).unwrap();
    store.set_audit_signer(key, marker);
    store.verify_integrity(&chain).unwrap().problems
}

fn cleanup(path: &str) {
    for suffix in ["", ".server_key", ".server_key.pub", ".server_key.audit_since", ".server_key.audit_since.tmp"] {
        std::fs::remove_file(format!("{path}{suffix}")).ok();
    }
}

#[test]
fn rewrite_with_recomputed_hashes_is_invisible_without_signature() {
    // Le trou d'avant, fige : sans signature, l'attaque passe.
    let path = temp_db("rewrite_unsigned");
    let hashes = seed(&path);
    rewrite_last_message_consistently(&path, &hashes[2]);
    assert!(problems(&path).is_empty(), "{:?}", problems(&path));
    cleanup(&path);
}

#[test]
fn rewrite_with_recomputed_hashes_is_caught_by_the_server_signature() {
    let path = temp_db("rewrite_signed");
    let hashes = seed_signed(&path);
    assert!(signed_problems(&path).is_empty());
    assert!(CstlNativeServer::try_with_data_path(0, &path).is_ok());
    rewrite_last_message_consistently(&path, &hashes[2]);
    let p = signed_problems(&path);
    assert!(p.iter().any(|m| m.contains("seq=2") && m.contains("signature du serveur invalide")), "{p:?}");
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("historique d'audit compromis"), "{err}");
    cleanup(&path);
}

#[test]
fn stripped_signature_is_detected() {
    let path = temp_db("stripped");
    seed_signed(&path);
    Connection::open(&path).unwrap().execute("UPDATE audit_trail SET server_signature=NULL", []).unwrap();
    let p = signed_problems(&path);
    assert_eq!(p.iter().filter(|m| m.contains("NON signe")).count(), 3, "{p:?}");
    cleanup(&path);
}

#[test]
fn replaced_server_key_or_forged_marker_refuses_startup() {
    // Cle remplacee : le marqueur "signe depuis" ne se verifie plus.
    let path = temp_db("rekey");
    seed_signed(&path);
    std::fs::remove_file(format!("{path}.server_key")).unwrap();
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("falsifie"), "{err}");
    cleanup(&path);

    // Marqueur reecrit pour faire passer tout l'historique pour "ancien".
    let path = temp_db("marker");
    seed_signed(&path);
    std::fs::write(format!("{path}.server_key.audit_since"), "since=999\nsignature=00\n").unwrap();
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("falsifie"), "{err}");
    cleanup(&path);
}

#[test]
fn legacy_unsigned_history_still_starts_and_new_entries_must_be_signed() {
    let path = temp_db("legacy");
    seed(&path); // 3 maillons d'avant la signature
    let server = CstlNativeServer::try_with_data_path(0, &path).expect("historique ancien accepte");
    drop(server);
    let key = Arc::new(ServerKey::for_data_path(&path).unwrap());
    let marker = key.read_audit_marker().unwrap().expect("marqueur cree au premier demarrage");
    assert_eq!(marker.since, 3, "le marqueur garde la valeur ecrite au premier demarrage");
    let mut store = AdnStore::open(&path).unwrap();
    store.set_audit_signer(key, marker);
    let report = store.verify_integrity(&store.load_chain().unwrap()).unwrap();
    assert_eq!((report.signature_legacy, report.signature_verified, report.problems.len()), (3, 0, 0), "{report:?}");
    cleanup(&path);
}

// -- Derniers maillons supprimes (2026-11-20) --------------------------------
// Supprimer la FIN de la chaine ne casse ni un lien ni une signature : tout
// ce qui reste est intact. Seul le marqueur signe du dernier maillon ecrit,
// tenu hors de la base, le montre.

#[test]
fn deleted_last_links_are_detected_and_startup_refused() {
    let path = temp_db("truncate");
    seed_signed(&path);
    assert!(signed_problems(&path).is_empty());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
    c.execute("DELETE FROM audit_trail WHERE seq >= 1", []).unwrap();
    drop(c);
    let p = signed_problems(&path);
    assert!(p.iter().any(|m| m.contains("derniers maillons supprimes") && m.contains("seq=2")), "{p:?}");
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("historique d'audit compromis"), "{err}");
    cleanup(&path);
}

#[test]
fn deleted_marker_with_signed_history_refuses_startup() {
    let path = temp_db("nomarker");
    seed_signed(&path);
    std::fs::remove_file(format!("{path}.server_key.audit_since")).unwrap();
    let err = CstlNativeServer::try_with_data_path(0, &path).err().expect("demarrage doit etre refuse");
    assert!(err.contains("absent"), "{err}");
    cleanup(&path);
}

#[test]
fn marker_behind_the_chain_is_accepted() {
    // Arret entre l'ecriture en base et celle du marqueur : la chaine est
    // plus LONGUE que le marqueur. Rien n'est perdu, ce n'est pas une alerte.
    let path = temp_db("behind");
    let hashes = seed_signed(&path);
    let key = ServerKey::for_data_path(&path).unwrap();
    key.write_audit_marker(&cstl_parser::server::server_key::AuditMarker { since: 0, head: Some((1, hashes[1].clone())) }).unwrap();
    assert!(signed_problems(&path).is_empty(), "{:?}", signed_problems(&path));
    cleanup(&path);
}

#[test]
fn first_version_marker_is_upgraded_then_protects_the_tail() {
    let path = temp_db("upgrade");
    seed_signed(&path);
    // Marqueur au format d'avant (seuil seul, sans dernier maillon).
    let key = ServerKey::for_data_path(&path).unwrap();
    let sig = key.sign_hex(b"cstl-audit-since-v1\nsince=0");
    std::fs::write(format!("{path}.server_key.audit_since"), format!("since=0\nsignature={sig}\n")).unwrap();
    drop(CstlNativeServer::try_with_data_path(0, &path).expect("ancien format accepte"));
    let marker = key.read_audit_marker().unwrap().unwrap();
    assert_eq!(marker.head.as_ref().map(|h| h.0), Some(2), "{marker:?}");
    // Desormais, supprimer la fin est detecte.
    Connection::open(&path).unwrap().execute("DELETE FROM audit_trail WHERE seq = 2", []).unwrap();
    assert!(signed_problems(&path).iter().any(|m| m.contains("derniers maillons supprimes")));
    cleanup(&path);
}

#[test]
fn marker_for_an_empty_chain_round_trips_and_a_wiped_database_restarts() {
    // Bug trouve par examples/agent_registry_persistence_test.rs : un
    // marqueur cree sur une chaine VIDE etait relu comme falsifie.
    let path = temp_db("empty");
    drop(CstlNativeServer::try_with_data_path(0, &path).expect("demarrage #1"));
    drop(CstlNativeServer::try_with_data_path(0, &path).expect("demarrage #2 sur la meme base vide"));
    cleanup(&path);

    // Base supprimee, cle et marqueur conserves : nouveau depart, pas un refus.
    let path = temp_db("wiped");
    seed_signed(&path);
    std::fs::remove_file(&path).unwrap();
    drop(CstlNativeServer::try_with_data_path(0, &path).expect("base supprimee : nouveau depart"));
    let key = ServerKey::for_data_path(&path).unwrap();
    assert_eq!(key.read_audit_marker().unwrap(), Some(cstl_parser::server::server_key::AuditMarker { since: 0, head: None }));
    cleanup(&path);
}
