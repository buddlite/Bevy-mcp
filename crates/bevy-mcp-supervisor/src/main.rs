use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bevy_mcp_server::AgentBevyMcpServer;
use bevy_mcp_server::tools::BevyMcpState;
use bevy_mcp_supervisor::{
    CargoExecutor, CargoExecutorConfig, LaunchSpec, ProcessManager, ProcessManagerConfig,
    SupervisorMcpServer, SupervisorPermissions, SupervisorTransport, generate_instance_id,
    generate_token,
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
    #[arg(long, value_name = "DIR", default_value = ".")]
    project_dir: PathBuf,
    #[arg(long, default_value_t = 20)]
    ready_timeout_secs: u64,
    #[arg(long, default_value_t = 3)]
    stop_grace_secs: u64,
    #[arg(long, default_value_t = 120)]
    check_timeout_secs: u64,
    #[arg(long, default_value_t = 300)]
    build_timeout_secs: u64,
    #[arg(long, default_value_t = 300)]
    test_timeout_secs: u64,
    #[arg(long)]
    deny_cargo_check: bool,
    #[arg(long)]
    deny_cargo_build: bool,
    #[arg(long)]
    deny_cargo_test: bool,
    #[arg(long)]
    deny_process_lifecycle: bool,
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
    let permissions = SupervisorPermissions {
        cargo_check: !cli.deny_cargo_check,
        cargo_build: !cli.deny_cargo_build,
        cargo_test: !cli.deny_cargo_test,
        process_launch: !cli.deny_process_lifecycle,
        process_stop: !cli.deny_process_lifecycle,
        process_restart: !cli.deny_process_lifecycle,
    };

    let mut cargo_config = CargoExecutorConfig::new(cli.project_dir.clone());
    cargo_config.check_timeout = Duration::from_secs(cli.check_timeout_secs);
    cargo_config.build_timeout = Duration::from_secs(cli.build_timeout_secs);
    cargo_config.test_timeout = Duration::from_secs(cli.test_timeout_secs);
    cargo_config.permissions = permissions;
    let cargo = CargoExecutor::initialize(cargo_config).await;
    if let Some(error) = cargo.initialization_error() {
        tracing::warn!(
            code = error.code,
            message = %error.message,
            project_dir = %cli.project_dir.display(),
            "Cargo project discovery is unavailable; build tools will return this error"
        );
    }

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
            "No managed executable configured; external supervisor bridge mode remains available:"
        );
        eprintln!("  BEVY_MCP_SUPERVISOR_ADDR={}", transport.address());
        eprintln!("  BEVY_MCP_SUPERVISOR_TOKEN={token}");
        eprintln!("  BEVY_MCP_INSTANCE_ID={instance_id}");
    }

    let state = BevyMcpState::from_backend(Arc::new(transport.backend()));
    let base = AgentBevyMcpServer::new(state);
    let service = SupervisorMcpServer::new(base, manager.clone(), cargo, permissions)
        .serve(stdio())
        .await?;
    let service_result = service.waiting().await;
    if let Err(error) = manager.shutdown_owned().await {
        tracing::error!(code = error.code, message = %error.message, "failed to clean up managed game during supervisor shutdown");
    }
    service_result?;
    Ok(())
}
