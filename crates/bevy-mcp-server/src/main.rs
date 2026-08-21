use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_server::tools::{BevyMcpServer, BevyMcpState};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("bevy_mcp_server=debug".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();

    let state = BevyMcpState::new(ingress, results);

    let server = BevyMcpServer::new(state).serve(stdio()).await?;

    server.waiting().await?;
    Ok(())
}
