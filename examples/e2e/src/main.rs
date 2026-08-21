use bevy::prelude::*;
use bevy_mcp_core::queue::{McpIngressQueue, McpResultQueue};
use bevy_mcp_host::BevyMcpPlugin;
use bevy_mcp_server::tools::{BevyMcpServer, BevyMcpState};
use rmcp::{ServiceExt, transport::stdio};

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
struct Player {
    name: String,
    health: f32,
}

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
struct Enemy;

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
struct Speed(f32);

#[derive(Component, Reflect, Default)]
#[reflect(Component)]
enum Faction {
    #[default]
    Neutral,
    Friendly,
    Hostile,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("bevy_mcp_server=debug")
        .with_writer(std::io::stderr)
        .init();

    // Create the shared queues.
    let ingress = McpIngressQueue::default();
    let results = McpResultQueue::default();

    // Create the MCP server state.
    let state = BevyMcpState::embedded(ingress.clone(), results.clone());

    // Spawn Bevy on a background thread.
    let bevy_ingress = ingress.clone();
    let bevy_results = results.clone();
    std::thread::spawn(move || {
        App::new()
            .add_plugins(MinimalPlugins)
            .add_plugins(BevyMcpPlugin::new().with_queues(bevy_ingress, bevy_results))
            .register_type::<Player>()
            .register_type::<Enemy>()
            .register_type::<Speed>()
            .register_type::<Faction>()
            .add_systems(Startup, setup)
            .add_systems(Update, tick_counter)
            .run();
    });

    // Give Bevy a moment to initialize.
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Run the MCP server on the main thread (owns stdin).
    let server = BevyMcpServer::new(state)
        .serve(stdio())
        .await
        .expect("MCP server failed to start");

    server.waiting().await.expect("MCP server error");
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Player {
            name: "Alice".into(),
            health: 100.0,
        },
        Speed(5.0),
        Name::new("Player"),
    ));
    commands.spawn((Enemy, Speed(3.0), Name::new("Goblin"), Faction::Hostile));
    commands.spawn((Enemy, Speed(7.0), Name::new("Dragon")));
}

fn tick_counter(mut frame: Local<u32>) {
    *frame += 1;
}
