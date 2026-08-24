use std::sync::Arc;

use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::tools::BevyMcpState;
use bevy_mcp_supervisor::{SupervisorTransport, generate_instance_id, generate_token};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("bevy_mcp_supervisor=debug".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let instance_id = generate_instance_id();
    let token = generate_token();
    let transport = SupervisorTransport::bind(instance_id.clone(), token.clone()).await?;

    eprintln!("bevy-mcp supervisor listening on {}", transport.address());
    eprintln!("Start a Stage-1 bridge-enabled game with:");
    eprintln!("  BEVY_MCP_SUPERVISOR_ADDR={}", transport.address());
    eprintln!("  BEVY_MCP_SUPERVISOR_TOKEN={token}");
    eprintln!("  BEVY_MCP_INSTANCE_ID={instance_id}");

    let state = BevyMcpState::from_backend(Arc::new(transport.backend()));
    let server = AgentBevyMcpServer::new(state).serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
