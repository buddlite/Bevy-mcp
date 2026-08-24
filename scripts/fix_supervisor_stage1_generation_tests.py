from pathlib import Path

path = Path("crates/bevy-mcp-supervisor/src/backend.rs")
text = path.read_text()

anchor = '''    async fn fake_hello(
        address: std::net::SocketAddr,
        token: &str,
        instance_id: &str,
    ) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let hello = WireEnvelope::new(WireMessage::Hello(Hello {
            token: token.to_string(),
            instance_id: instance_id.to_string(),
            host_version: "test".into(),
            bevy_version: None,
            pid: None,
        }));
        write_envelope(&mut stream, &hello, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        let response = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        match response.message {
            WireMessage::HelloAccepted(accepted) => (stream, accepted.connection_id),
            other => panic!("unexpected handshake response: {other:?}"),
        }
    }
'''

helpers = anchor + '''
    async fn wait_for_host_state(
        backend: &SupervisorBackend,
        expected: HostState,
        timeout: Duration,
    ) {
        let observed = tokio::time::timeout(timeout, async {
            loop {
                let actual = backend.snapshot().host;
                if actual == expected {
                    return actual;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        assert_eq!(
            observed.unwrap_or_else(|_| panic!(
                "timed out waiting for host state {expected:?}; snapshot: {:?}",
                backend.snapshot()
            )),
            expected
        );
    }

    async fn wait_for_transport_state(
        backend: &SupervisorBackend,
        expected: TransportState,
        timeout: Duration,
    ) {
        let observed = tokio::time::timeout(timeout, async {
            loop {
                let actual = backend.snapshot().transport;
                if actual == expected {
                    return actual;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        assert_eq!(
            observed.unwrap_or_else(|_| panic!(
                "timed out waiting for transport state {expected:?}; snapshot: {:?}",
                backend.snapshot()
            )),
            expected
        );
    }
'''

if anchor not in text:
    raise SystemExit("fake_hello anchor not found")
text = text.replace(anchor, helpers, 1)

old_sleep = '''        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(transport.backend().snapshot().host, HostState::Ready);
'''
new_wait = '''        wait_for_host_state(
            &transport.backend(),
            HostState::Ready,
            Duration::from_millis(250),
        )
        .await;
'''
if old_sleep not in text:
    raise SystemExit("fixed-sleep readiness assertion not found")
text = text.replace(old_sleep, new_wait, 1)

insert_before = '''    #[tokio::test]
    async fn simultaneous_second_game_is_rejected() {
'''
new_tests = '''    #[tokio::test]
    async fn pending_request_fails_immediately_when_connection_generation_disconnects() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let backend = transport.backend();
        let (mut stream, connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;

        // Consume the automatic readiness probe without acknowledging it so the host remains Waiting.
        let probe = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert!(matches!(
            probe.message,
            WireMessage::Command(WireCommand {
                command: McpCommand::HostProbe { .. },
                ..
            })
        ));

        let call_backend = backend.clone();
        let required_connection_id = connection_id.clone();
        let call = tokio::spawn(async move {
            call_backend
                .call_on_connection(
                    McpCommand::WorldSummary,
                    Duration::from_secs(2),
                    Some(&required_connection_id),
                    true,
                )
                .await
        });

        let command = read_envelope(&mut stream, DEFAULT_MAX_FRAME_SIZE)
            .await
            .unwrap();
        assert!(matches!(
            command.message,
            WireMessage::Command(WireCommand {
                command: McpCommand::WorldSummary,
                ..
            })
        ));

        drop(stream);
        let error = tokio::time::timeout(Duration::from_millis(250), call)
            .await
            .expect("pending call was not failed promptly on disconnect")
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "GAME_DISCONNECTED");
        wait_for_transport_state(
            &backend,
            TransportState::Disconnected,
            Duration::from_millis(250),
        )
        .await;
    }

    #[tokio::test]
    async fn reconnect_after_transport_loss_gets_a_new_connection_generation() {
        let transport = SupervisorTransport::bind_with_options(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let backend = transport.backend();
        let (first, first_connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        drop(first);

        wait_for_transport_state(
            &backend,
            TransportState::Disconnected,
            Duration::from_millis(250),
        )
        .await;

        let (_second, second_connection_id) =
            fake_hello(transport.address(), "secret", "run-test").await;
        assert_ne!(first_connection_id, second_connection_id);
        assert_eq!(
            backend.snapshot().connection_id.as_deref(),
            Some(second_connection_id.as_str())
        );
    }

    #[tokio::test]
    async fn stale_generation_response_cannot_resolve_current_pending_request() {
        let backend = SupervisorBackend::new(
            "run-test".into(),
            "secret".into(),
            DEFAULT_MAX_FRAME_SIZE,
            Duration::from_secs(1),
        );
        let request_id = 77;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        backend.inner.pending.lock().unwrap().insert(
            request_id,
            PendingRequest {
                connection_id: "conn-current".into(),
                sender,
            },
        );

        backend.route_response(
            "conn-stale",
            WireResponse {
                request_id,
                result: McpResult::success(serde_json::json!({ "source": "stale" })),
            },
        );
        assert!(backend.inner.pending.lock().unwrap().contains_key(&request_id));

        backend.route_response(
            "conn-current",
            WireResponse {
                request_id,
                result: McpResult::success(serde_json::json!({ "source": "current" })),
            },
        );
        let result = receiver.await.unwrap().unwrap();
        match result {
            McpResult::Success(value) => {
                assert_eq!(value.get("source").and_then(|value| value.as_str()), Some("current"));
            }
            other => panic!("expected current-generation success, got {other:?}"),
        }
    }

''' + insert_before

if insert_before not in text:
    raise SystemExit("test insertion anchor not found")
text = text.replace(insert_before, new_tests, 1)

path.write_text(text)
print("Stage 1 readiness race fixed and generation isolation tests added")
