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
pub async fn accept_connections(
    listener: TcpListener,
    ctx: Arc<ServerContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let (socket, addr) = listener.accept().await?;
        eprintln!("[Server] New connection from {}", addr);

        let ctx = ctx.clone();
        tokio::spawn(async move {
            if let Err(e) = handler::handle_connection(socket, ctx).await {
                eprintln!("[Server] Error handling connection: {}", e);
            }
        });
    }
}
