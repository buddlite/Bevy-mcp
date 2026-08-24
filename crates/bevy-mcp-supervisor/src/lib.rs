pub mod backend;
pub mod cargo_executor;
pub mod permissions;
pub mod process_manager;
pub mod process_tools;

pub use backend::{
    HostState, ProcessObservation, SupervisorBackend, SupervisorSnapshot, SupervisorTransport,
    TransportState, generate_instance_id, generate_token,
};
pub use cargo_executor::{
    CargoError, CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationKind,
    CargoOperationSnapshot, CargoOperationState, CargoRunResult,
};
pub use permissions::SupervisorPermissions;
pub use process_manager::{
    LaunchSpec, ProcessError, ProcessLogEntry, ProcessManager, ProcessManagerConfig,
    ProcessOwnership, ProcessSnapshot, ProcessState,
};
pub use process_tools::SupervisorMcpServer;
