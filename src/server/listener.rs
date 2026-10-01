/// TCP Listener for CSTL payloads
use tokio::net::TcpListener;
use std::sync::Arc;
use super::handler;
use super::ServerContext;

pub async fn create_listener(addr: &str) -> Result<TcpListener, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    Ok(listener)
}

/// Accepte les connexions TCP entrantes et les traite avec le contexte serveur.
///
/// `tls_acceptor`: `None` -- comportement inchange depuis le debut de la
/// session, TCP brut, `handle_connection` monomorphise sur `TcpStream`. Si
/// `Some` (TLS 1.3 active, voir `server/tls.rs` + `server/mod.rs::start`),
/// chaque connexion acceptee passe d'abord par un handshake TLS
/// (`acceptor.accept(socket)`) AVANT d'atteindre `handle_connection`, qui
/// recoit alors un `tokio_rustls::server::TlsStream<TcpStream>` --
/// `handle_connection` est generique sur le transport (voir handler.rs) et
/// ne fait aucune difference entre les deux a partir de ce point.
/// Un handshake TLS qui echoue (client sans certificat alors que
/// l'authentification mutuelle est exigee, certificat non reconnu, client
/// qui ne parle pas TLS du tout) est loggue et la connexion abandonnee --
/// jamais un fallback silencieux vers du texte clair.
pub async fn accept_connections(
    listener: TcpListener,
    ctx: Arc<ServerContext>,
    tls_acceptor: Option<Arc<tokio_rustls::TlsAcceptor>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (socket, addr) = listener.accept().await?;
        eprintln!("[Server] New connection from {}", addr);

        let ctx = ctx.clone();
        match &tls_acceptor {
            Some(acceptor) => {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor.accept(socket).await {
                        Ok(tls_stream) => {
                            if let Err(e) = handler::handle_connection(tls_stream, ctx).await {
                                eprintln!("[Server] Error handling TLS connection from {}: {}", addr, e);
                            }
                        }
                        Err(e) => {
                            eprintln!("[Server] TLS handshake failed for {}: {}", addr, e);
                        }
                    }
                });
            }
            None => {
                tokio::spawn(async move {
                    if let Err(e) = handler::handle_connection(socket, ctx).await {
                        eprintln!("[Server] Error handling connection: {}", e);
                    }
                });
            }
        }
    }
}
