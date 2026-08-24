use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::tools::BevyMcpState;
use bevy_mcp_supervisor::{
    LaunchSpec, ProcessManager, ProcessManagerConfig, SupervisorMcpServer, SupervisorTransport,
    generate_instance_id, generate_token,
};
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "bevy-mcp", about = "Persistent MCP supervisor for Bevy games")]
struct Cli {
    #[arg(long, value_name = "PATH")]
    game_executable: Option<PathBuf>,
    #[arg(long = "game-arg", value_name = "ARG")]
    game_args: Vec<String>,
    #[arg(long, value_name = "DIR")]
    game_cwd: Option<PathBuf>,
    #[arg(long, default_value_t = 20)]
    ready_timeout_secs: u64,
    #[arg(long, default_value_t = 3)]
    stop_grace_secs: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("bevy_mcp_supervisor=debug".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let has_managed_target = cli.game_executable.is_some();
    let instance_id = generate_instance_id();
    let token = generate_token();
    let transport = SupervisorTransport::bind(instance_id.clone(), token.clone()).await?;

    let launch = cli.game_executable.map(|executable| {
        let mut spec = LaunchSpec::new(executable).args(cli.game_args);
        if let Some(cwd) = cli.game_cwd {
            spec = spec.current_dir(cwd);
        }
        spec
    });
    let manager = ProcessManager::new(
        transport.backend(),
        transport.address(),
        token.clone(),
        ProcessManagerConfig {
            launch,
            ready_timeout: Duration::from_secs(cli.ready_timeout_secs),
            graceful_stop_timeout: Duration::from_secs(cli.stop_grace_secs),
            ..Default::default()
        },
    );

    eprintln!("bevy-mcp supervisor listening on {}", transport.address());
    if !has_managed_target {
        eprintln!(
            "No managed executable configured; external Stage-1 bridge mode remains available:"
        );
        eprintln!("  BEVY_MCP_SUPERVISOR_ADDR={}", transport.address());
        eprintln!("  BEVY_MCP_SUPERVISOR_TOKEN={token}");
        eprintln!("  BEVY_MCP_INSTANCE_ID={instance_id}");
    }

    let state = BevyMcpState::from_backend(Arc::new(transport.backend()));
    let base = AgentBevyMcpServer::new(state);
    let service = SupervisorMcpServer::new(base, manager.clone())
        .serve(stdio())
        .await?;
    let service_result = service.waiting().await;
    if let Err(error) = manager.shutdown_owned().await {
        tracing::error!(code = error.code, message = %error.message, "failed to clean up managed game during supervisor shutdown");
    }
    service_result?;
    Ok(())
}
