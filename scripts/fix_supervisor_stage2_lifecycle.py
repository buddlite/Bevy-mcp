from pathlib import Path

path = Path("crates/bevy-mcp-supervisor/src/process_manager.rs")
text = path.read_text()


def replace_once(old, new, label):
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one occurrence, found {count}")
    text = text.replace(old, new, 1)

# One lifecycle transition at a time. The child mutex protects the slot; this lock protects
# multi-await operations such as launch readiness and graceful stop from racing one another.
replace_once(
    '''    config: ProcessManagerConfig,
    child: AsyncMutex<Option<ManagedChild>>,
    record: Mutex<ProcessRecord>,''',
    '''    config: ProcessManagerConfig,
    lifecycle: AsyncMutex<()>,
    child: AsyncMutex<Option<ManagedChild>>,
    record: Mutex<ProcessRecord>,''',
    "Inner lifecycle field",
)
replace_once(
    '''                config,
                child: AsyncMutex::new(None),
                record: Mutex::new(ProcessRecord::default()),''',
    '''                config,
                lifecycle: AsyncMutex::new(()),
                child: AsyncMutex::new(None),
                record: Mutex::new(ProcessRecord::default()),''',
    "Inner lifecycle init",
)

# Public methods own lifecycle arbitration; private inner methods compose without recursive locks.
replace_once(
    '''    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self''',
    '''    pub async fn launch(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.launch_inner().await
    }

    async fn launch_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let launch = self''',
    "launch wrapper",
)
replace_once(
    '''    pub async fn stop(&self) -> Result<ProcessSnapshot, ProcessError> {
        let child_present = self.inner.child.lock().await.is_some();''',
    '''    pub async fn stop(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        self.stop_inner().await
    }

    async fn stop_inner(&self) -> Result<ProcessSnapshot, ProcessError> {
        let child_present = self.inner.child.lock().await.is_some();''',
    "stop wrapper",
)

old_restart = '''    pub async fn restart(&self) -> Result<ProcessSnapshot, ProcessError> {
        let ownership = self.inner.record.lock().unwrap().ownership;
        let child_present = self.inner.child.lock().await.is_some();
        if !child_present && self.inner.backend.snapshot().transport == TransportState::Connected {
            return Err(ProcessError::new(
                "PROCESS_NOT_MANAGED",
                "The connected game was not launched by this supervisor and cannot be restarted",
            ));
        }
        if !child_present && ownership != ProcessOwnership::Managed {
            return Err(ProcessError::new(
                "PROCESS_NOT_RUNNING",
                "No managed game process has been launched",
            ));
        }
        if child_present {
            self.stop().await?;
        }
        self.launch().await
    }

    pub async fn shutdown_owned(&self) -> Result<(), ProcessError> {
        if self.inner.child.lock().await.is_none() {
            return Ok(());
        }
        self.stop().await.map(|_| ())
    }
'''
new_restart = '''    pub async fn restart(&self) -> Result<ProcessSnapshot, ProcessError> {
        let _operation = self.try_lifecycle_operation()?;
        let ownership = self.inner.record.lock().unwrap().ownership;
        let child_present = self.inner.child.lock().await.is_some();
        if !child_present && self.inner.backend.snapshot().transport == TransportState::Connected {
            return Err(ProcessError::new(
                "PROCESS_NOT_MANAGED",
                "The connected game was not launched by this supervisor and cannot be restarted",
            ));
        }
        if !child_present && ownership != ProcessOwnership::Managed {
            return Err(ProcessError::new(
                "PROCESS_NOT_RUNNING",
                "No managed game process has been launched",
            ));
        }
        if child_present {
            self.stop_inner().await?;
        }
        self.launch_inner().await
    }

    pub async fn shutdown_owned(&self) -> Result<(), ProcessError> {
        // Supervisor teardown waits for an in-flight lifecycle operation instead of racing it.
        let _operation = self.inner.lifecycle.lock().await;
        if self.inner.child.lock().await.is_none() {
            return Ok(());
        }
        self.stop_inner().await.map(|_| ())
    }

    fn try_lifecycle_operation(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, ProcessError> {
        self.inner.lifecycle.try_lock().map_err(|_| {
            ProcessError::new(
                "PROCESS_OPERATION_IN_PROGRESS",
                "Another process lifecycle operation is already in progress",
            )
        })
    }
'''
replace_once(old_restart, new_restart, "restart/shutdown lifecycle composition")

# Explicit lexical scope ensures the non-Send std MutexGuard cannot cross the await.
replace_once(
    '''            if backend.instance_id == instance_id && backend.host == HostState::Ready {
                let mut record = self.inner.record.lock().unwrap();
                if record.instance_id.as_deref() == Some(instance_id) {
                    record.state = ProcessState::Running;
                }
                drop(record);
                return Ok(self.status().await);
            }''',
    '''            if backend.instance_id == instance_id && backend.host == HostState::Ready {
                {
                    let mut record = self.inner.record.lock().unwrap();
                    if record.instance_id.as_deref() == Some(instance_id) {
                        record.state = ProcessState::Running;
                    }
                }
                return Ok(self.status().await);
            }''',
    "startup readiness guard scope",
)

# Strengthen hang classification: observe connected transport + unresponsive Bevy host before timeout.
old_hang = '''    #[tokio::test]
    async fn host_hang_remains_connected_but_unresponsive() {
        let (_transport, manager) = fixture_manager("hang", Duration::from_secs(2)).await;
        let launch = manager.launch().await.unwrap_err();
        assert_eq!(launch.code, "PROCESS_START_TIMEOUT");
        // The launch timeout cleans the managed process, while the backend's probe semantics
        // are independently covered by Stage 1. This test ensures a hung host is classified
        // by host readiness rather than a spurious protocol/transport error.
        assert!(launch.message.contains("host-ready"));
    }
'''
new_hang = '''    #[tokio::test]
    async fn host_hang_remains_connected_but_unresponsive() {
        let (_transport, manager) = fixture_manager("hang", Duration::from_millis(500)).await;
        let launch_manager = manager.clone();
        let launch = tokio::spawn(async move { launch_manager.launch().await });

        tokio::time::timeout(Duration::from_millis(400), async {
            loop {
                let backend = manager.backend().snapshot();
                if backend.transport == TransportState::Connected
                    && backend.host == HostState::Unresponsive
                {
                    let status = manager.status().await;
                    assert_eq!(status.state, ProcessState::Starting);
                    assert_eq!(status.transport, "connected");
                    assert_eq!(status.host, "unresponsive");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("hung fixture never reached connected/unresponsive classification");

        let error = launch.await.unwrap().unwrap_err();
        assert_eq!(error.code, "PROCESS_START_TIMEOUT");
    }
'''
replace_once(old_hang, new_hang, "hang classification test")

# Verify lifecycle arbitration under a deliberately slow readiness path.
insert_anchor = '''    #[tokio::test]
    async fn restart_rotates_instance_identity() {'''
concurrency_test = '''    #[tokio::test]
    async fn concurrent_lifecycle_operations_are_rejected_deterministically() {
        let (_transport, manager) = fixture_manager("slow_ready", Duration::from_secs(2)).await;
        let first_manager = manager.clone();
        let first_launch = tokio::spawn(async move { first_manager.launch().await });

        tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if manager.status().await.state == ProcessState::Starting {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first lifecycle operation never entered starting state");

        let second_launch = manager.launch().await.unwrap_err();
        assert_eq!(second_launch.code, "PROCESS_OPERATION_IN_PROGRESS");
        let concurrent_stop = manager.stop().await.unwrap_err();
        assert_eq!(concurrent_stop.code, "PROCESS_OPERATION_IN_PROGRESS");

        let started = first_launch.await.unwrap().unwrap();
        assert_eq!(started.state, ProcessState::Running);
        manager.stop().await.unwrap();
    }

''' + insert_anchor
replace_once(insert_anchor, concurrency_test, "concurrency test insertion")

# The Stage 1 fixture probe timeout is 150 ms. Keep the delayed response visibly slow while
# remaining inside that deadline so this tests readiness gating rather than timeout handling.
replace_once(
    '''                            std::thread::sleep(Duration::from_millis(180));''',
    '''                            std::thread::sleep(Duration::from_millis(80));''',
    "slow readiness delay",
)
replace_once(
    '''        assert!(started.elapsed() >= Duration::from_millis(150));''',
    '''        assert!(started.elapsed() >= Duration::from_millis(60));''',
    "slow readiness elapsed assertion",
)

# AsyncGroupChild::try_wait can report the group leader's exit while descendants are still alive
# on Unix. Poll the leader explicitly, then retain the process-group / Job Object handle long
# enough to terminate and drain every remaining owned descendant before clearing the child slot.
old_reap = '''    async fn reap_if_exited(&self) -> Result<bool, ProcessError> {
        let mut guard = self.inner.child.lock().await;
        let Some(managed) = guard.as_mut() else {
            return Ok(true);
        };
        let instance_id = managed.instance_id.clone();
        match managed.child.try_wait() {
            Ok(Some(status)) => {
                guard.take();
                drop(guard);
                self.record_exit(&instance_id, status);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => Err(ProcessError::new(
                "PROCESS_STATUS_FAILED",
                format!("Failed to poll managed process state: {error}"),
            )),
        }
    }
'''
new_reap = '''    async fn reap_if_exited(&self) -> Result<bool, ProcessError> {
        let mut guard = self.inner.child.lock().await;
        let Some(managed) = guard.as_mut() else {
            return Ok(true);
        };
        let instance_id = managed.instance_id.clone();
        let status = match managed.child.inner().try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return Ok(false),
            Err(error) => {
                return Err(ProcessError::new(
                    "PROCESS_STATUS_FAILED",
                    format!("Failed to poll managed process leader state: {error}"),
                ));
            }
        };

        // The group leader may exit while owned descendants remain alive. Keep ownership of
        // the process-group / Job Object until every remaining member has been terminated and
        // drained. start_kill may report that the group is already gone; that is already clean.
        let _ = managed.child.start_kill();
        managed.child.wait().await.map_err(|error| {
            ProcessError::new(
                "PROCESS_STATUS_FAILED",
                format!("Failed to drain managed process tree after leader exit: {error}"),
            )
        })?;

        guard.take();
        drop(guard);
        self.record_exit(&instance_id, status);
        Ok(true)
    }
'''
replace_once(old_reap, new_reap, "leader-aware process-tree reap")

path.write_text(text)
print("Stage 2 lifecycle, readiness, and process-tree cleanup fixes applied")
