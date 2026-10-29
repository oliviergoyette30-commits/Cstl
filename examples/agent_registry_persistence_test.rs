/// examples/agent_registry_persistence_test.rs -- verification live du fix
/// 2026-10-29 de la faille de continuite d'identite trouvee par l'audit du
/// 15 octobre (voir ADDENDUM_VERIFICATION_REPO_REEL_2026-10-15.md, section
/// "Prise de controle du conseil") : `AgentRegistry` vivait uniquement en
/// memoire, vide a chaque redemarrage du serveur (`AgentRegistry::new()`
/// dans `try_with_data_path`), donc le premier client qui se reenregistrait
/// sous un nom deja pris APRES un restart en devenait le proprietaire --
/// cle publique comprise, sans jamais avoir a prouver la possession de
/// l'ancienne cle (le check de rotation dans handler.rs compare contre le
/// registre en memoire, qui etait vide).
///
/// Ce test demarre un vrai `CstlNativeServer` (meme fichier SQLite),
/// enregistre alice avec sa cle, DROP le serveur (simule un arret propre),
/// puis en construit un second sur le MEME fichier (simule le redemarrage)
/// et verifie qu'alice -- et surtout sa cle publique -- est toujours la
/// AVANT que quiconque n'envoie le moindre message.
use cstl_parser::server::CstlNativeServer;

fn make_db_path() -> String {
    std::env::temp_dir()
        .join(format!("cstl_agent_persistence_test_{}.db", std::process::id()))
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::main]
async fn main() {
    let db_path = make_db_path();
    let _ = std::fs::remove_file(&db_path);

    println!("=== Demarrage #1: enregistrement d'alice avec sa cle publique ===");
    {
        let server = CstlNativeServer::try_with_data_path(0, &db_path).expect("demarrage #1");
        server
            .adn_store
            .lock()
            .await
            .save_agent(
                "alice",
                "5.0.0",
                &["communication".to_string()],
                0.95,
                Some("alice_real_pubkey_hex"),
                1_000,
            )
            .expect("save_agent");

        let in_memory_after_save = {
            let reg = server.agent_registry.read().await;
            reg.agents.iter().find(|a| a.name == "alice").map(|a| a.public_key.clone())
        };
        println!("registre en memoire juste apres save_agent (pas encore relu): {:?}", in_memory_after_save);
        // `server` est droppe ici -- "arret" du serveur. Le registre en
        // memoire disparait avec lui; seule la base SQLite sur disque
        // survit.
    }

    println!("\n=== Demarrage #2 (meme fichier '{}'): simule un redemarrage ===", db_path);
    let server2 = CstlNativeServer::try_with_data_path(0, &db_path).expect("demarrage #2");
    let restored_pubkey = {
        let reg = server2.agent_registry.read().await;
        reg.agents.iter().find(|a| a.name == "alice").map(|a| a.public_key.clone())
    };
    println!("cle publique d'alice restauree au redemarrage (AVANT tout message reseau): {:?}", restored_pubkey);

    assert_eq!(
        restored_pubkey,
        Some(Some("alice_real_pubkey_hex".to_string())),
        "AVANT le fix 2026-10-29, ceci etait None -- le registre redemarrait toujours vide. \
         Un attaquant pouvait alors se reenregistrer sous 'alice' avec SA PROPRE cle, sans \
         preuve de rotation, puisque rien n'existait en memoire contre quoi verifier."
    );

    println!(
        "\n!!! CONFIRME: la cle publique d'alice survit au redemarrage -- la garde de rotation \
        dans handler.rs (qui compare contre ce registre) protege maintenant aussi contre un \
        redemarrage, pas seulement contre la session en cours."
    );

    let _ = std::fs::remove_file(&db_path);
}
