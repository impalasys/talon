// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use crate::control::{config::proto as config, resources::ResourceStore, scheduling, topics};
use crate::gateway::{
    rpc::{proto, GrpcGatewayHandler},
    server::Gateway,
};
use crate::test_support::{EnvVarGuard, RecordingPubSub};
use crate::worker::{
    fanout::FanoutHub, mcp_registry::McpRegistry, scheduler_auth::SchedulerRequestAuthenticator,
    session_control::SessionCancellationRegistry, WorkerEventHandler,
};
use anyhow::ensure;
use base64::{engine::general_purpose::STANDARD, Engine};
use prost::Message;
use std::collections::HashMap;

const NS: &str = "Tenant:openai-live:main";
const OTHER: &str = "Tenant:openai-other:main";
const AGENT: &str = "luna-test";
const LONG_INPUT: &str =
    "Write the integers from 1 to 2000, one per line. Continue until instructed otherwise.";

fn spec() -> manifests::AgentSpec {
    manifests::AgentSpec {
        runtime:Some(manifests::AgentRuntime { kind:"openai_agents".into(), ..Default::default() }),
        system_prompt:"Follow user instructions precisely, including new instructions during your answer. If asked for a remembered code that was never supplied, reply UNKNOWN.".into(),
        model_policy:Some(manifests::ModelPolicy { profiles:vec![manifests::ModelProfile {
            name:"default".into(), model:Some(manifests::Model { provider:"openai".into(), name:"gpt-5.6-luna".into(),
                thinking:Some(manifests::ThinkingConfig { enabled:false, ..Default::default() }), ..Default::default() })
        }] }), ..Default::default()
    }
}

struct Live {
    cp: Arc<ControlPlane>,
    pubsub: Arc<RecordingPubSub>,
    config: Arc<Config>,
    rpc: GrpcGatewayHandler,
}

impl Live {
    async fn new(database: &str) -> Result<Self> {
        let kv = Arc::new(crate::control::kv::SqliteKvStore::new(database, "talon_live").await?);
        let pubsub = Arc::new(RecordingPubSub::default());
        let cp = Arc::new(ControlPlane::builder(kv, pubsub.clone()).build());
        let rpc = GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, cp.as_ref().clone())),
        };
        let config = Arc::new(Config {
            providers: HashMap::from([(
                "openai".into(),
                config::LlmProviderConfig {
                    config: Some(config::llm_provider_config::Config::Openai(
                        config::OpenAiConfig {
                            model: "this-default-must-not-be-used".into(),
                            ..Default::default()
                        },
                    )),
                },
            )]),
            ..Default::default()
        });
        Ok(Self {
            cp,
            pubsub,
            config,
            rpc,
        })
    }

    fn worker(&self) -> WorkerEventHandler {
        WorkerEventHandler {
            cp: self.cp.clone(),
            config: self.config.clone(),
            mcp_registry: Arc::new(McpRegistry::new()),
            scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
            worker_id: "openai-live-worker".into(),
            fanout_hub: Arc::new(FanoutHub::new()),
            session_cancellations: Arc::new(SessionCancellationRegistry::default()),
        }
    }

    async fn seed(&self, ns: &str, key: &str) -> Result<()> {
        let store = ResourceStore::new(self.cp.kv.clone(), self.cp.pubsub.clone());
        store
            .upsert(
                ns,
                manifests::Resource {
                    kind: "Agent".into(),
                    metadata: Some(manifests::ResourceMeta {
                        name: AGENT.into(),
                        namespace: ns.into(),
                        ..Default::default()
                    }),
                    spec: Some(manifests::ResourceSpec {
                        kind: Some(manifests::resource_spec::Kind::Agent(spec())),
                    }),
                    ..Default::default()
                },
            )
            .await?;
        let root = resolver::tenant_root_namespace(ns).unwrap();
        store
            .upsert(
                &root,
                manifests::Resource {
                    kind: "Secret".into(),
                    metadata: Some(manifests::ResourceMeta {
                        name: "api-keys".into(),
                        namespace: root.clone(),
                        ..Default::default()
                    }),
                    spec: Some(manifests::ResourceSpec {
                        kind: Some(manifests::resource_spec::Kind::Secret(
                            manifests::SecretSpec {
                                data: HashMap::from([("openai".into(), STANDARD.encode(key))]),
                                ..Default::default()
                            },
                        )),
                    }),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn runtime(&self, ns: &str, session: &str) -> Result<OpenAiAgentRuntime> {
        OpenAiAgentRuntime::build(self.cp.clone(), &self.config, ns, AGENT, session, &spec()).await
    }

    async fn send(
        &self,
        ns: &str,
        session: &str,
        text: &str,
    ) -> Result<crate::control::events::SessionDispatchEvent> {
        self.rpc
            .handle_send_message(tonic::Request::new(proto::SendMessageRequest {
                ns: ns.into(),
                agent: AGENT.into(),
                session_id: session.into(),
                message: text.into(),
                labels: HashMap::new(),
            }))
            .await?;
        self.last_dispatch().await
    }

    async fn last_dispatch(&self) -> Result<crate::control::events::SessionDispatchEvent> {
        let published = self.pubsub.published.lock().await;
        let (_, bytes) = published
            .iter()
            .rev()
            .find(|(topic, _)| topic == topics::SESSION_DISPATCH_TOPIC)
            .context("No dispatch event")?;
        Ok(crate::control::events::SessionDispatchEvent::decode(
            bytes.as_slice(),
        )?)
    }

    async fn submit(
        &self,
        session: &str,
        id: &str,
        text: &str,
    ) -> Result<crate::control::events::SessionDispatchEvent> {
        self.rpc
            .handle_submit_session_turn(tonic::Request::new(proto::SubmitSessionTurnRequest {
                ns: NS.into(),
                agent: AGENT.into(),
                session_id: session.into(),
                message: Some(data_proto::SessionMessage {
                    id: id.into(),
                    role: data_proto::MessageRole::RoleUser as i32,
                    parts: vec![data_proto::SessionMessagePart {
                        part_type: data_proto::SessionMessagePartType::Text as i32,
                        content: text.into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                labels: HashMap::new(),
            }))
            .await?;
        self.last_dispatch().await
    }

    async fn run(&self, event: crate::control::events::SessionDispatchEvent) -> Result<()> {
        tokio::time::timeout(
            Duration::from_secs(150),
            self.worker().handle_session_message(event),
        )
        .await??;
        Ok(())
    }

    async fn receipt(
        &self,
        event: &crate::control::events::SessionDispatchEvent,
    ) -> Result<AcceptedInput> {
        self.input(event, true).await
    }

    async fn input(
        &self,
        event: &crate::control::events::SessionDispatchEvent,
        require_turn: bool,
    ) -> Result<AcceptedInput> {
        let rt = self.runtime(&event.ns, &event.session_id).await?;
        tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                if let Some(input) = rt
                    .read::<AcceptedInput>("OpenAIInput", &event.submission_id)
                    .await?
                {
                    if !require_turn || !input.turn_id.is_empty() {
                        return Ok(input);
                    }
                }
                if let Ok(submission) = self.submission(event).await {
                    ensure!(
                        !sessions::submission_is_terminal(&submission),
                        "Input failed before attribution: {submission:?}"
                    );
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?
    }

    async fn submission(
        &self,
        event: &crate::control::events::SessionDispatchEvent,
    ) -> Result<data_proto::SessionSubmission> {
        self.cp
            .kv
            .get_msg(&keys::session_submission(
                &event.ns,
                &event.agent,
                &event.session_id,
                &event.submission_id,
            ))
            .await?
            .context("Missing submission")
    }

    async fn reply(&self, event: &crate::control::events::SessionDispatchEvent) -> Result<String> {
        let sub = self.submission(event).await?;
        ensure!(
            sub.status == data_proto::SessionSubmissionStatus::Committed as i32,
            "Submission did not commit: {sub:?}"
        );
        let message: data_proto::SessionMessage = self
            .cp
            .kv
            .get_msg(&keys::session_message(
                &event.ns,
                &event.agent,
                &event.session_id,
                sub.committed_message_id
                    .as_deref()
                    .context("No committed reply")?,
            ))
            .await?
            .context("No persisted reply")?;
        Ok(scheduling::session_message_text_projection(&message))
    }

    async fn control_server(
        &self,
        worker: &WorkerEventHandler,
        socket: &std::path::Path,
    ) -> Result<(
        CancellationToken,
        tokio::task::JoinHandle<std::result::Result<(), tonic::transport::Error>>,
    )> {
        let listener = tokio::net::UnixListener::bind(socket)?;
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let service = crate::gateway::rpc::worker_proto::session_control_service_server::SessionControlServiceServer::new(
            crate::worker::session_control::SessionControlServiceImpl::new(worker.session_cancellations.clone()));
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::UnixListenerStream::new(listener),
                    stop.cancelled_owned(),
                )
                .await
        });
        self.cp
            .kv
            .set_msg(
                &keys::ResourceKey::new(
                    crate::control::ns::TALON_SYSTEM,
                    &[],
                    "Worker",
                    &worker.worker_id,
                ),
                &crate::gateway::rpc::resources_proto::Worker {
                    status: Some(crate::gateway::rpc::resources_proto::WorkerStatus {
                        phase: "ready".into(),
                        endpoints: vec![crate::gateway::rpc::resources_proto::WorkerEndpoint {
                            url: format!("unix://{}", socket.display()),
                            protocol: "grpc".into(),
                            audience: String::new(),
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await?;
        Ok((shutdown, server))
    }
}

#[test]
fn rejects_unsupported_configuration_and_input() {
    let example = crate::control::manifest::parse_agent(include_str!(
        "../../../manifests/examples/openai-agents/agent.yaml"
    ))
    .unwrap();
    crate::harness::agents::resolver::validate_agent_spec(example.spec.as_ref().unwrap()).unwrap();
    assert!(is_openai_agents(example.spec.as_ref().unwrap()));
    let mut agent = spec();
    assert!(validate_spec(&agent).is_ok());
    agent.mcp_server_refs.push("tools".into());
    assert!(validate_spec(&agent).is_err());
    let image = data_proto::SessionMessage {
        role: data_proto::MessageRole::RoleUser as i32,
        parts: vec![data_proto::SessionMessagePart {
            part_type: data_proto::SessionMessagePartType::Image as i32,
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(validate_message(&image).is_err());
}

#[tokio::test]
async fn clear_rejects_concurrent_openai_input() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("clear.db"));
    let live = Live::new(&database).await?;
    live.seed(NS, "unused-offline").await?;
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    let pause = EnvVarGuard::set("TALON_OPENAI_TEST_PAUSE", "clear_locked");
    let _reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
    let rpc = live.rpc.clone();
    let id = session.clone();
    let clearing = tokio::spawn(async move {
        rpc.handle_clear_session(tonic::Request::new(proto::ClearSessionRequest {
            ns: NS.into(),
            agent: AGENT.into(),
            session_id: id,
        }))
        .await
    });
    wait_for_pause("clear_locked").await?;
    let submitted = live
        .submit(&session, "during-clear", "must not be accepted")
        .await;
    let sent = live
        .send(NS, &session, "must not be queued during clear")
        .await;
    let queued = crate::control::session_queue::queue_text_message(
        live.cp.kv.as_ref(),
        NS,
        AGENT,
        &session,
        crate::control::session_queue::NEXT_QUEUE,
        "connector input during clear",
        HashMap::new(),
        chrono::Utc::now(),
    )
    .await;
    drop(pause);
    let cleared = clearing.await?;
    ensure!(
        submitted.is_err() && sent.is_err() && queued.is_err(),
        "Input was accepted while ClearSession held its lock: submit={}, send={}, queue={}",
        submitted.is_ok(),
        sent.is_ok(),
        queued.is_ok()
    );
    cleared?;
    let response = live
        .rpc
        .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
            ns: NS.into(),
            agent: AGENT.into(),
            session_id: session,
            ..Default::default()
        }))
        .await?
        .into_inner();
    ensure!(
        response.state == "IDLE" && response.messages.is_empty(),
        "Clear did not leave an empty idle session"
    );
    Ok(())
}

async fn wait_for_pause(point: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while std::env::var("TALON_OPENAI_TEST_REACHED").ok().as_deref() != Some(point) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn completing_worker_cannot_release_input_before_its_submission_is_saved() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let _ttl = EnvVarGuard::set("TALON_SESSION_PROCESSING_TIMEOUT_SECONDS", "1");
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("completion.db"));
    let mut live = Live::new(&database).await?;
    live.seed(NS, "unused-offline").await?;
    // A real worker completes with a configuration error before making HTTP calls.
    Arc::get_mut(&mut live.config).unwrap().providers.clear();
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    let first = live.submit(&session, "first-input", "first").await?;
    let pause = EnvVarGuard::set("TALON_OPENAI_TEST_PAUSE", "release_checked");
    let _reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
    let worker = live.worker();
    let completing = tokio::spawn(async move { worker.handle_session_message(first).await });
    wait_for_pause("release_checked").await?;
    std::env::set_var("TALON_OPENAI_TEST_PAUSE", "release_checked,input_reserved");
    let next = Live::new(&database).await?;
    let id = session.clone();
    let admitting = tokio::spawn(async move { next.submit(&id, "next-input", "next").await });
    wait_for_pause("input_reserved").await?;
    std::env::set_var("TALON_OPENAI_TEST_PAUSE", "input_reserved");
    completing.await??;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let state = live
        .rpc
        .handle_get_session(tonic::Request::new(proto::GetSessionRequest {
            ns: NS.into(),
            agent: AGENT.into(),
            session_id: session.clone(),
            ..Default::default()
        }))
        .await?
        .into_inner()
        .state;
    let cleared = live
        .rpc
        .handle_clear_session(tonic::Request::new(proto::ClearSessionRequest {
            ns: NS.into(),
            agent: AGENT.into(),
            session_id: session.clone(),
        }))
        .await;
    drop(pause);
    let next = admitting.await??;
    ensure!(
        state == "PROCESSING" && cleared.is_err(),
        "Completing worker released admitted input: state={state}, clear_allowed={}",
        cleared.is_ok()
    );
    ensure!(
        !sessions::submission_is_terminal(&live.submission(&next).await?),
        "New input disappeared"
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_input_cannot_take_over_an_unfinished_admission() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("duplicate.db"));
    let live = Live::new(&database).await?;
    live.seed(NS, "unused-offline").await?;
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    let pause = EnvVarGuard::set("TALON_OPENAI_TEST_PAUSE", "input_reserved");
    let _reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
    let first = Live::new(&database).await?;
    let id = session.clone();
    let writing = tokio::spawn(async move { first.submit(&id, "same-id", "original").await });
    wait_for_pause("input_reserved").await?;
    let original = live.cp.kv.get(&keys::session(NS, AGENT, &session)).await?;
    let duplicate = tokio::time::timeout(
        Duration::from_millis(200),
        live.submit(&session, "same-id", "conflicting retry"),
    )
    .await;
    let after = live.cp.kv.get(&keys::session(NS, AGENT, &session)).await?;
    drop(pause);
    writing.await??;
    ensure!(
        matches!(duplicate, Ok(Err(_))) && original == after,
        "A duplicate took over another caller's unfinished admission"
    );
    Ok(())
}

#[tokio::test]
async fn rejected_retry_of_terminal_input_does_not_leave_session_busy() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("retry.db"));
    let mut live = Live::new(&database).await?;
    live.seed(NS, "unused-offline").await?;
    Arc::get_mut(&mut live.config).unwrap().providers.clear();
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    let event = live.submit(&session, "same-id", "original").await?;
    live.run(event.clone()).await?;
    ensure!(live
        .submit(&session, "same-id", "changed payload")
        .await
        .is_err());
    live.rpc
        .handle_clear_session(tonic::Request::new(proto::ClearSessionRequest {
            ns: NS.into(),
            agent: AGENT.into(),
            session_id: session,
        }))
        .await
        .context("A rejected terminal retry left the session busy")?;
    live.run(event.clone()).await?;
    ensure!(
        live.cp
            .kv
            .get(&keys::session_submission(
                NS,
                AGENT,
                &event.session_id,
                &event.submission_id
            ))
            .await?
            .is_none(),
        "Broker redelivery recreated a cleared submission"
    );
    Ok(())
}

#[tokio::test]
async fn transport_retries_preserve_input_and_drain_while_busy() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    use crate::control::session_queue as queue;
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("transport.db"));
    let live = Live::new(&database).await?;
    // This check exercises storage and dispatch only; it makes no HTTP calls.
    live.seed(NS, "unused-offline").await?;
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    live.cp
        .kv
        .set_msg(
            &keys::session(NS, AGENT, &session),
            &data_proto::Session {
                id: session.clone(),
                agent: AGENT.into(),
                ns: NS.into(),
                status: "PROCESSING".into(),
                last_active: chrono::Utc::now().timestamp_micros(),
                ..Default::default()
            },
        )
        .await?;
    let message = data_proto::SessionMessage {
        id: "stable-input".into(),
        role: data_proto::MessageRole::RoleUser as i32,
        parts: vec![data_proto::SessionMessagePart {
            part_type: data_proto::SessionMessagePartType::Text as i32,
            content: "same input".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let key = keys::session_message(NS, AGENT, &session, &message.id);
    persist_input(live.cp.kv.as_ref(), &key, &message).await?;
    persist_input(live.cp.kv.as_ref(), &key, &message).await?;
    let mut conflicting = message.clone();
    conflicting.parts[0].content = "different input".into();
    assert!(persist_input(live.cp.kv.as_ref(), &key, &conflicting)
        .await
        .is_err());
    queue::queue_session_message(
        live.cp.kv.as_ref(),
        NS,
        AGENT,
        &session,
        queue::NEXT_QUEUE,
        message,
        chrono::Utc::now(),
    )
    .await?;
    queue::queue_text_message(
        live.cp.kv.as_ref(),
        NS,
        AGENT,
        &session,
        queue::NEXT_QUEUE,
        "second input",
        HashMap::new(),
        chrono::Utc::now(),
    )
    .await?;
    queue::dispatch_next_queued_message(
        live.cp.kv.as_ref(),
        live.pubsub.as_ref(),
        NS,
        AGENT,
        &session,
        queue::NEXT_QUEUE,
        chrono::Utc::now(),
    )
    .await?;
    assert!(live
        .cp
        .kv
        .list_keys(
            &keys::session_queue_prefix(NS, AGENT, &session, queue::NEXT_QUEUE),
            None
        )
        .await?
        .is_empty());
    let published = live.pubsub.published.lock().await;
    let events: Vec<_> = published
        .iter()
        .filter(|(topic, _)| topic == topics::SESSION_DISPATCH_TOPIC)
        .map(|(_, bytes)| {
            crate::control::events::SessionDispatchEvent::decode(bytes.as_slice()).unwrap()
        })
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].submission_id, "stable-input");
    assert_ne!(events[0].submission_id, events[1].submission_id);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the real OpenAI Agents API and bills OPENAI_API_KEY"]
async fn live_luna_stop_survives_worker_crash() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let key = std::env::var("OPENAI_API_KEY").context("Set OPENAI_API_KEY to run the live test")?;
    let _base = EnvVarGuard::remove("OPENAI_BASE_URL");
    let _ttl = EnvVarGuard::set("TALON_SESSION_PROCESSING_TIMEOUT_SECONDS", "2");
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("stop.db"));
    let live = Live::new(&database).await?;
    live.seed(NS, &key).await?;
    let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
    let event = live.submit(&session, "stopped-input", LONG_INPUT).await?;
    let worker = live.worker();
    let (shutdown, server) = live
        .control_server(&worker, &directory.path().join("stop.sock"))
        .await?;
    let _shutdown_guard = shutdown.clone().drop_guard();
    let mut pause = Some(EnvVarGuard::set(
        "TALON_OPENAI_TEST_PAUSE",
        "before_snapshot",
    ));
    let _reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
    let w = worker.clone();
    let e = event.clone();
    let mut running = tokio::spawn(async move { w.handle_session_message(e).await });
    let result = async {
        wait_for_pause("before_snapshot").await?;
        live.rpc
            .handle_stop_session_generation(tonic::Request::new(
                proto::StopSessionGenerationRequest {
                    ns: NS.into(),
                    agent: AGENT.into(),
                    session_id: session.clone(),
                },
            ))
            .await?;
        let runtime = live.runtime(NS, &session).await?;
        ensure!(runtime.read::<bool>("OpenAICancel", &event.submission_id).await? == Some(true),
            "Stop was acknowledged before its durable request was saved");
        // The gateway has acknowledged Stop, but no OpenAI cancel was sent.
        running.abort();
        let _ = (&mut running).await;
        drop(pause.take());
        tokio::time::sleep(Duration::from_millis(2300)).await;
        let restarted = Live::new(&database).await?;
        restarted.run(event.clone()).await?;
        let submission = restarted.submission(&event).await?;
        let input: AcceptedInput = runtime.read("OpenAIInput", &event.submission_id).await?.context("Missing accepted input")?;
        let turns = runtime.get(&format!("/{}/turns?order=desc&limit=1", input.session_id)).await?;
        ensure!(
            submission.status == data_proto::SessionSubmissionStatus::Interrupted as i32,
            "Unexpected Stop recovery: local={submission:?}, remote={}, error={}, cancel_saved={:?}",
            turns["data"][0]["status"], turns["data"][0]["error"],
            runtime.read::<bool>("OpenAICancel", &event.submission_id).await?
        );
        let input = restarted.receipt(&event).await?;
        let runtime = restarted.runtime(NS, &session).await?;
        ensure!(
            runtime
                .get(&format!("/{}/turns/{}", input.session_id, input.turn_id))
                .await?["status"]
                == "cancelled",
            "Recovery did not forward the saved Stop to OpenAI"
        );
        eprintln!("PASS: acknowledged Stop survives a worker crash and reaches real OpenAI");
        let previous_turn = input.turn_id;
        let next = live.submit(&session, "stopped-again", LONG_INPUT).await?;
        pause = Some(EnvVarGuard::set("TALON_OPENAI_TEST_PAUSE", "before_snapshot"));
        let _reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
        let w = worker.clone();
        let e = next.clone();
        running = tokio::spawn(async move { w.handle_session_message(e).await });
        wait_for_pause("before_snapshot").await?;
        live.rpc
            .handle_stop_session_generation(tonic::Request::new(
                proto::StopSessionGenerationRequest {
                    ns: NS.into(),
                    agent: AGENT.into(),
                    session_id: session.clone(),
                },
            ))
            .await?;
        drop(pause.take());
        tokio::time::timeout(Duration::from_secs(150), &mut running).await???;
        let input = live.receipt(&next).await?;
        ensure!(input.turn_id != previous_turn, "A fresh Stop was attributed to the preceding cancelled turn");
        ensure!(live.submission(&next).await?.status == data_proto::SessionSubmissionStatus::Interrupted as i32,
            "The fresh Stop did not interrupt its own turn");
        ensure!(runtime.get(&format!("/{}/turns/{}", input.session_id, input.turn_id)).await?["status"] == "cancelled",
            "The new OpenAI turn was not cancelled");
        eprintln!("PASS: Stop after a prior cancellation targets the new remote turn");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    running.abort();
    drop(pause);
    shutdown.cancel();
    server.await??;
    let runtime = live.runtime(NS, &session).await?;
    if let Some(state) = runtime
        .read::<RemoteSession>("OpenAIRuntime", "session")
        .await?
    {
        let id = match state.session_id {
            Some(id) => Some(id),
            None => runtime.find_created_session(&state.nonce).await?,
        };
        if let Some(id) = id {
            let response = runtime
                .request(Method::DELETE, &format!("/{id}"))
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await?;
            if response.status() != reqwest::StatusCode::NOT_FOUND {
                ensure!(
                    OpenAiAgentRuntime::checked(response)
                        .await?
                        .json::<Value>()
                        .await?["deleted"]
                        == true,
                    "OpenAI did not confirm test session deletion"
                );
            }
        }
    }
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the real OpenAI Agents API and bills OPENAI_API_KEY"]
async fn live_luna_lifecycle_and_crash_recovery() -> Result<()> {
    let _env = crate::test_support::async_env_mutex().lock().await;
    let key = std::env::var("OPENAI_API_KEY").context("Set OPENAI_API_KEY to run the live test")?;
    let _base = EnvVarGuard::remove("OPENAI_BASE_URL");
    let _ttl = EnvVarGuard::set("TALON_SESSION_PROCESSING_TIMEOUT_SECONDS", "2");
    let directory = tempfile::tempdir()?;
    let database = crate::control::kv::sqlite_url_for_path(&directory.path().join("live.db"));
    let live = Live::new(&database).await?;
    live.seed(NS, &key).await?;
    let mut sessions_to_clean = Vec::<(String, String)>::new();
    let result = async {
        let session = scheduling::create_session(&live.cp, NS, AGENT).await?;
        sessions_to_clean.push((NS.into(),session.clone()));
        let first = live.send(NS, &session, "Remember code ELM-742. Reply only READY.").await?;
        live.run(first.clone()).await?;
        ensure!(live.reply(&first).await?.trim() == "READY", "Wrong first answer");
        let first_input = live.receipt(&first).await?;
        let rt = live.runtime(NS, &session).await?;
        let remote = rt.get(&format!("/{}", first_input.session_id)).await?;
        ensure!(remote["agent"]["model"] == "gpt-5.6-luna", "Manifest model was not used");
        eprintln!("PASS: real Luna first turn persisted through Talon");

        let restarted = Live::new(&database).await?;
        let follow = restarted.send(NS, &session, "What code did I ask you to remember? Reply with the code only.").await?;
        let _disconnect = EnvVarGuard::set("TALON_OPENAI_TEST_DISCONNECT", "1");
        restarted.run(follow.clone()).await?;
        drop(_disconnect);
        ensure!(restarted.reply(&follow).await?.trim() == "ELM-742", "Restart lost context");
        ensure!(restarted.receipt(&follow).await?.session_id == first_input.session_id, "Follow-up created a new remote session");
        live.run(follow.clone()).await?;
        let after_replay = rt.items(&first_input.session_id, None).await?;
        ensure!(after_replay.iter().filter(|i| i["role"] == "user").count() == 2, "Redelivery duplicated remote input");
        eprintln!("PASS: restart, actual stream disconnect and redelivery preserve context without duplicate input");

        let busy = live.send(NS, &session, LONG_INPUT).await?;
        let worker = live.worker();
        let w = worker.clone(); let e = busy.clone();
        let running = tokio::spawn(async move { w.handle_session_message(e).await });
        let busy_input = live.receipt(&busy).await?;
        let steering = live.send(NS, &session, "Stop the list now. Reply only STEER_ONE.").await?;
        let second_worker = live.worker(); let e = steering.clone();
        let second = tokio::spawn(async move { second_worker.handle_session_message(e).await });
        tokio::time::timeout(Duration::from_secs(20), live.input(&steering, false)).await??;
        // The first steering input may still be queued inside OpenAI. Talon
        // must pass another input through SubmitTurn immediately.
        let third_event = live.submit(&session, "steer-again", "Reply only STEER_OK instead.").await?;
        ensure!(third_event.submission_id == "steer-again", "Talon held SubmitTurn behind remote generation");
        let w = live.worker(); let e = third_event.clone();
        let third = tokio::spawn(async move { w.handle_session_message(e).await });
        tokio::time::timeout(Duration::from_secs(20), live.input(&third_event, false)).await??;
        let steering_input = live.receipt(&steering).await?;
        ensure!(busy_input.turn_id == steering_input.turn_id, "Second message did not steer the active OpenAI turn");
        ensure!(busy_input.turn_id == live.receipt(&third_event).await?.turn_id, "Third message did not reach the active OpenAI turn");
        tokio::time::timeout(Duration::from_secs(120), running).await???;
        tokio::time::timeout(Duration::from_secs(120), second).await???;
        tokio::time::timeout(Duration::from_secs(120), third).await???;
        ensure!(live.reply(&steering).await?.trim() == "STEER_OK", "Steering was not reflected in the final answer");
        ensure!(live.submission(&busy).await?.committed_message_id == live.submission(&steering).await?.committed_message_id,
            "Steering generated duplicate Talon replies");
        ensure!(live.submission(&busy).await?.committed_message_id == live.submission(&third_event).await?.committed_message_id,
            "SubmitTurn generated duplicate Talon replies");
        eprintln!("PASS: repeated busy input reaches OpenAI through SendMessage and SubmitTurn, with one final reply");

        let cancellable = live.send(NS, &session, LONG_INPUT).await?;
        let worker = live.worker(); let w = worker.clone(); let e = cancellable.clone();
        let running = tokio::spawn(async move { w.handle_session_message(e).await });
        let input = live.receipt(&cancellable).await?;
        let queued_cancel = live.send(NS, &session, "Keep listing the integers.").await?;
        let worker = live.worker(); let w = worker.clone(); let e = queued_cancel.clone();
        let queued_running = tokio::spawn(async move { w.handle_session_message(e).await });
        let queued_input = tokio::time::timeout(Duration::from_secs(20), live.input(&queued_cancel, false)).await??;
        ensure!(queued_input.item_id.is_empty(), "Queued cancellation test missed the unapplied-input window");
        let (shutdown, server) = live.control_server(&worker, &directory.path().join("control.sock")).await?;
        let _shutdown_guard = shutdown.clone().drop_guard();
        live.rpc.handle_stop_session_generation(tonic::Request::new(proto::StopSessionGenerationRequest {
            ns:NS.into(), agent:AGENT.into(), session_id:session.clone()
        })).await?;
        tokio::time::timeout(Duration::from_secs(120), running).await???;
        tokio::time::timeout(Duration::from_secs(120), queued_running).await???;
        shutdown.cancel(); server.await??;
        ensure!(live.submission(&cancellable).await?.status == data_proto::SessionSubmissionStatus::Interrupted as i32, "Cancellation was not recorded as interrupted");
        ensure!(live.submission(&queued_cancel).await?.status == data_proto::SessionSubmissionStatus::Interrupted as i32,
            "Cancelled queued input was left pending");
        ensure!(rt.get(&format!("/{}/turns/{}",input.session_id,input.turn_id)).await?["status"] == "cancelled", "OpenAI did not cancel");
        let after_cancel = live.send(NS, &session, "What code did I ask you to remember? Reply with the code only.").await?;
        live.run(after_cancel.clone()).await?;
        ensure!(live.reply(&after_cancel).await?.trim() == "ELM-742", "Cancelled input broke later conversation continuity");
        eprintln!("PASS: gateway stop cancels active and queued input; the next message retains context");

        let abandoned = live.submit(&session, "recover-observer", "Reply only RECOVERED.").await?;
        let exit = EnvVarGuard::set("TALON_OPENAI_TEST_OBSERVER_EXIT", "1");
        let error = live.run(abandoned.clone()).await.unwrap_err();
        drop(exit);
        ensure!(error.downcast_ref::<RecoveryNeeded>().is_some(), "Lost observer was reported as remote failure");
        let claim = live.submission(&abandoned).await?;
        ensure!(!sessions::submission_is_terminal(&claim) && claim.claim_expires_at.unwrap_or_default() <= chrono::Utc::now().timestamp_micros(),
            "Recoverable error left an active lease blocking immediate retry");
        let stop_error = live.rpc.handle_stop_session_generation(tonic::Request::new(proto::StopSessionGenerationRequest {
            ns:NS.into(), agent:AGENT.into(), session_id:session.clone()
        })).await.unwrap_err();
        ensure!(stop_error.code() == tonic::Code::Unavailable, "Stop falsely succeeded while the OpenAI observer was absent");
        let redelivery = live.submit(&session, "recover-observer", "Reply only RECOVERED.").await?;
        ensure!(redelivery.submission_id == abandoned.submission_id, "Client retry did not redrive the original observer");
        live.run(redelivery).await?;
        ensure!(live.reply(&abandoned).await?.trim() == "RECOVERED", "Client retry failed to recover output");
        let input = live.receipt(&abandoned).await?;
        ensure!(rt.items(&input.session_id,None).await?.iter().filter(|item| item["role"] == "user" && item_text(item) == "Reply only RECOVERED.").count() == 1,
            "Client retry duplicated an accepted OpenAI input");
        eprintln!("PASS: SubmitTurn with the same message ID immediately recovers an abandoned observer");

        for point in ["accepted", "accepted_followup", "completed", "committed"] {
            let fresh = scheduling::create_session(&live.cp, NS, AGENT).await?;
            sessions_to_clean.push((NS.into(),fresh.clone()));
            if point == "accepted_followup" {
                let initial = live.send(NS, &fresh, "Reply only READY.").await?;
                live.run(initial).await?;
            }
            let event = live.send(NS, &fresh, "Reply only CRASH_OK.").await?;
            let pause_at = if point == "accepted_followup" { "accepted" } else { point };
            let pause = EnvVarGuard::set("TALON_OPENAI_TEST_PAUSE", pause_at);
            let reached = EnvVarGuard::remove("TALON_OPENAI_TEST_REACHED");
            let w = live.worker(); let e = event.clone();
            let task = tokio::spawn(async move { w.handle_session_message(e).await });
            tokio::time::timeout(Duration::from_secs(90), async {
                while std::env::var("TALON_OPENAI_TEST_REACHED").ok().as_deref() != Some(pause_at) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }).await?;
            task.abort(); let _ = task.await;
            drop(pause); drop(reached);
            tokio::time::sleep(Duration::from_millis(2300)).await;
            let restarted = Live::new(&database).await?;
            restarted.run(event.clone()).await?;
            ensure!(restarted.reply(&event).await?.trim() == "CRASH_OK", "Recovery failed at {point}");
            let input = restarted.receipt(&event).await?;
            let runtime = restarted.runtime(NS, &fresh).await?;
            let items = runtime.items(&input.session_id, None).await?;
            let expected = if point == "accepted_followup" { 2 } else { 1 };
            ensure!(items.iter().filter(|i| i["role"] == "user").count() == expected, "Crash recovery duplicated remote work at {point}");
            eprintln!("PASS: crash at {point} recovers against real OpenAI without resubmitting");
        }

        ensure!(resolver::resolve_openai_agents_credentials(&spec(), &live.config, &live.cp, OTHER).await.is_err(), "Tenant fell back to another tenant's credentials");
        live.seed(OTHER, &key).await?;
        live.cp.kv.set_msg(&keys::session(OTHER, AGENT, &session), &data_proto::Session {
            id:session.clone(), agent:AGENT.into(), ns:OTHER.into(), status:"IDLE".into(), ..Default::default()
        }).await?;
        sessions_to_clean.push((OTHER.into(),session.clone()));
        let isolated = live.send(OTHER, &session, "What code did I ask you to remember? Reply with the code or UNKNOWN only.").await?;
        live.run(isolated.clone()).await?;
        ensure!(live.reply(&isolated).await?.trim() == "UNKNOWN", "Another tenant received the first tenant's memory");
        ensure!(live.receipt(&isolated).await?.session_id != first_input.session_id, "Remote session mapping crossed tenants");
        eprintln!("PASS: tenant credentials and identically named sessions remain isolated");

        let failed_session = scheduling::create_session(&live.cp, NS, AGENT).await?;
        sessions_to_clean.push((NS.into(),failed_session.clone()));
        let mut invalid = spec();
        invalid.model_policy.as_mut().unwrap().profiles[0].model.as_mut().unwrap().name = "talon-live-invalid-model".into();
        ResourceStore::new(live.cp.kv.clone(),live.cp.pubsub.clone()).upsert(NS, manifests::Resource {
            kind:"Agent".into(), metadata:Some(manifests::ResourceMeta { name:AGENT.into(), namespace:NS.into(), ..Default::default() }),
            spec:Some(manifests::ResourceSpec { kind:Some(manifests::resource_spec::Kind::Agent(invalid)) }), ..Default::default()
        }).await?;
        let failed = live.send(NS, &failed_session, "Reply only SHOULD_NOT_SUCCEED.").await?;
        live.run(failed.clone()).await?;
        ensure!(live.submission(&failed).await?.status == data_proto::SessionSubmissionStatus::Failed as i32,
            "Real OpenAI rejection produced false success");
        live.seed(NS,&key).await?;
        eprintln!("PASS: real API rejection is persisted as failure");

        ensure!(scheduling::compact_session(live.cp.kv.as_ref(),live.cp.pubsub.as_ref(),NS,AGENT,&session,chrono::Utc::now()).await.is_err(), "Native compaction was accepted");
        let old_remote = first_input.session_id;
        live.rpc.handle_clear_session(tonic::Request::new(proto::ClearSessionRequest {
            ns:NS.into(), agent:AGENT.into(), session_id:session.clone()
        })).await?;
        let reset = live.send(NS, &session, "What code did I ask you to remember? Reply with the code or UNKNOWN only.").await?;
        live.run(reset.clone()).await?;
        ensure!(live.reply(&reset).await?.trim() == "UNKNOWN", "Clear retained remote context");
        ensure!(live.receipt(&reset).await?.session_id != old_remote, "Clear reused the old remote session");
        OpenAiAgentRuntime::checked(rt.request(Method::DELETE,&format!("/{old_remote}")).send().await?).await?;
        eprintln!("PASS: clear starts fresh remote context; native compaction is rejected");
        Ok::<_, anyhow::Error>(())
    }.await;

    // Only sessions created by this test are touched; no deployed Talon agent is used.
    let mut cleanup_errors = Vec::new();
    for (ns, session) in sessions_to_clean {
        let cleanup = async {
            let rt = live.runtime(&ns, &session).await?;
            if let Some(state) = rt.read::<RemoteSession>("OpenAIRuntime", "session").await? {
                let id = match state.session_id {
                    Some(id) => Some(id),
                    None => rt.find_created_session(&state.nonce).await?,
                };
                if let Some(id) = id {
                    let response = rt
                        .request(Method::DELETE, &format!("/{id}"))
                        .timeout(REQUEST_TIMEOUT)
                        .send()
                        .await?;
                    if response.status() != reqwest::StatusCode::NOT_FOUND {
                        let deleted: Value =
                            OpenAiAgentRuntime::checked(response).await?.json().await?;
                        ensure!(
                            deleted["deleted"] == true,
                            "OpenAI did not confirm deletion of {id}"
                        );
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = cleanup {
            cleanup_errors.push(format!("{ns}/{session}: {error:#}"));
        }
    }
    result?;
    ensure!(
        cleanup_errors.is_empty(),
        "Live session cleanup failed: {}",
        cleanup_errors.join("; ")
    );
    Ok(())
}
