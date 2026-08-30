/// Connection Handler - Wire Parser + Validator + Router
/// 
/// Flow:
/// 1. Receive TCP payload
/// 2. Parse as CSTL
/// 3. Validate constraints
/// 4. Route to destination agent
/// 5. Record audit trail
/// 6. Send response

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::agent_discovery::AgentRegistry;
use super::audit::HashChain;
use super::parser;
use super::validator;

pub async fn handle_connection(
    mut socket: TcpStream,
    registry: Arc<AgentRegistry>,
    chain: Arc<Mutex<HashChain>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = vec![0; 8192];
    
    loop {
        let n = socket.read(&mut buffer).await?;
        
        if n == 0 {
            eprintln!("[Handler] Connection closed");
            return Ok(());
        }
        
        let raw_payload = String::from_utf8_lossy(&buffer[..n]).to_string();
        eprintln!("[Handler] Received {} bytes", n);
        
        // STEP 1: Parse CSTL payload
        let parse_result = parser::parse_payload(&raw_payload);
        
        match parse_result {
            Ok(payload) => {
                eprintln!("[Handler] ✅ Parse successful");
                
                // STEP 2: Validate semantically
                let validation = validator::validate_payload(&payload);
                
                if validation.valid {
                    eprintln!("[Handler] ✅ Validation passed");

                    // AUDIT: le serveur (orchestrateur) calcule le vrai SHA-256.
                    // L'agent envoie PARENT_HASH=root; on le remplace ici.
                    let entry = { chain.lock().await.append(&payload) };
                    
                    // STEP 3: Extract routing info
                    let receiver = payload.intent.get("receiver").cloned().unwrap_or_else(|| "unknown".to_string());
                    let purpose = payload.intent.get("purpose").cloned().unwrap_or_else(|| "unknown".to_string());
                    
                    eprintln!("[Handler] 🔀 Routing to: {} (purpose: {})", receiver, purpose);
                    
                    // STEP 4: Try to route to agent
                    if let Some(agent) = registry.route("communication") {
                        eprintln!("[Handler] ✅ Found agent: {}", agent.name);
                        
                        // STEP 5: Build response
                        let response = format!(
                            "#!CSTL v5.0.0 MODE=A\n\
                            META [encoder=CstlNativeServer, produced_by=Server, status=processed]\n\
                            INTENT_PAYLOAD [purpose=acknowledgement, sender=server, receiver={}]\n\
                            RELATION [type=received, subject={}, status=valid]\n\
                            AUDIT [hash={}, parent_hash={}, seq={}]\n\
                            ---END---\n",
                            payload.intent.get("sender").cloned().unwrap_or_else(|| "unknown".to_string()),
                            purpose,
                            entry.hash,
                            entry.parent_hash,
                            entry.seq
                        );
                        
                        socket.write_all(response.as_bytes()).await?;
                        eprintln!("[Handler] ✅ Response sent");
                    } else {
                        let error_response = "#!CSTL v5.0.0 MODE=A\nINTENT_PAYLOAD [purpose=error, status=no_agent]\n---END---\n";
                        socket.write_all(error_response.as_bytes()).await?;
                        eprintln!("[Handler] ❌ No agent found");
                    }
                } else {
                    // Validation failed - send error response
                    eprintln!("[Handler] ❌ Validation failed: {} errors", validation.errors.len());
                    
                    let error_msg = validation.errors.iter()
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .collect::<Vec<_>>()
                        .join("; ");
                    
                    let error_response = format!(
                        "#!CSTL v5.0.0 MODE=A\n\
                        META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                        INTENT_PAYLOAD [purpose=validation_error, errors={}]\n\
                        ---END---\n",
                        error_msg.replace("\"", "\\\"")
                    );
                    
                    socket.write_all(error_response.as_bytes()).await?;
                }
            }
            Err(e) => {
                // Parse failed - send parse error
                eprintln!("[Handler] ❌ Parse failed: {}", e);
                
                let error_response = format!(
                    "#!CSTL v5.0.0 MODE=A\n\
                    META [encoder=CstlNativeServer, produced_by=Server, status=error]\n\
                    INTENT_PAYLOAD [purpose=parse_error, details={}]\n\
                    ---END---\n",
                    e.to_string().replace("\"", "\\\"")
                );
                
                socket.write_all(error_response.as_bytes()).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_module_compiles() {
        // Just verify the module compiles
        // Real testing requires async runtime
    }
}
