pub mod backend;
pub mod cargo_executor;
pub mod development_status;
pub mod permissions;
pub mod process_manager;
pub mod process_tools;
pub mod rebuild_restart;

#[cfg(test)]
mod cargo_executor_acceptance;
#[cfg(test)]
mod supervisor_acceptance;

pub use backend::{
    HostState, ProcessObservation, SupervisorBackend, SupervisorSnapshot, SupervisorTransport,
    TransportState, generate_instance_id, generate_token,
};
pub use cargo_executor::{
    CargoError, CargoExecutor, CargoExecutorConfig, CargoInvocation, CargoOperationKind,
    CargoOperationSnapshot, CargoOperationState, CargoRunResult,
};
pub use development_status::{
    DevelopmentFailure, DevelopmentGeneration, DevelopmentOperationRef, DevelopmentProjectStatus,
    DevelopmentState, DevelopmentStatus, RecoveryAction,
};
pub use permissions::SupervisorPermissions;
pub use process_manager::{
    LaunchSpec, ProcessError, ProcessLogEntry, ProcessManager, ProcessManagerConfig,
    ProcessOwnership, ProcessSnapshot, ProcessState,
};
pub use process_tools::SupervisorMcpServer;
pub use rebuild_restart::{
    RebuildRestartCoordinator, RebuildRestartError, RebuildRestartEvidence, RebuildRestartFailure,
    RebuildRestartSnapshot, RebuildRestartState,
};
