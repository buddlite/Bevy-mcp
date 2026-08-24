#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisorPermissions {
    pub cargo_check: bool,
    pub cargo_build: bool,
    pub cargo_test: bool,
    pub process_launch: bool,
    pub process_stop: bool,
    pub process_restart: bool,
}

impl SupervisorPermissions {
    pub const fn full() -> Self {
        Self {
            cargo_check: true,
            cargo_build: true,
            cargo_test: true,
            process_launch: true,
            process_stop: true,
            process_restart: true,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            cargo_check: false,
            cargo_build: false,
            cargo_test: false,
            process_launch: false,
            process_stop: false,
            process_restart: false,
        }
    }
}

impl Default for SupervisorPermissions {
    fn default() -> Self {
        Self::full()
    }
}
