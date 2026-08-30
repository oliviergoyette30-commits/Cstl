/// Connection handler for CSTL payloads
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use std::sync::Arc;
use crate::agent_discovery::AgentRegistry;

pub async fn handle_connection(
    mut socket: TcpStream,
    _registry: Arc<AgentRegistry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = vec![0; 4096];
    
    loop {
        let n = socket.read(&mut buffer).await?;
        
        if n == 0 {
            // Connection closed
            eprintln!("[Handler] Connection closed");
            return Ok(());
        }
        
        let payload = String::from_utf8_lossy(&buffer[..n]);
        eprintln!("[Handler] Received {} bytes", n);
        
        // TODO: Parse as CSTL
        // TODO: Validate semantically
        // TODO: Route to destination
        // TODO: Record in audit trail
        
        // Echo back for now (proof of concept)
        let response = format!("[Server] Received {} bytes\n", n);
        socket.write_all(response.as_bytes()).await?;
    }
}
