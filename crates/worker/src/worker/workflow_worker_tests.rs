// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::workflows::*;
use crate::test_support::MockKvStore;
use crate::test_support::RecordingPubSub;
use crate::worker::mcp_registry::McpRegistry;
use crate::worker::scheduler_auth::SchedulerRequestAuthenticator;
use crate::worker::WorkerEventHandler;
use ::control::control::config::proto;
use ::control::control::config::Config;
use ::control::control::config::ProviderConfig;
use ::control::control::config::Secret;
use ::control::control::events::MessageDirection;
use ::control::control::events::SessionDispatchEvent;
use ::control::control::events::WorkflowDispatchEvent;
use ::control::control::scheduler::ScheduleWakeupRequest;
use ::control::control::scheduler::ScheduledWakeup;
use ::control::control::scheduler::SchedulerBackend;
use ::control::control::ProtoKeyValueStoreExt;
use ::harness::knowledge::KnowledgeBook;
use ::harness::knowledge::KvKnowledgeBook;
use ::protocol::manifests;
use async_trait::async_trait;
use axum::{routing::post, Json, Router};
use prost::Message;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;

use ::test_support::workflows::*;
fn workflow_handler(kv: Arc<MockKvStore>, pubsub: Arc<RecordingPubSub>) -> WorkerEventHandler {
    workflow_handler_with_base_url(kv, pubsub, "https://unused.example.com".to_string())
}

fn workflow_handler_with_base_url(
    kv: Arc<MockKvStore>,
    pubsub: Arc<RecordingPubSub>,
    base_url: String,
) -> WorkerEventHandler {
    WorkerEventHandler {
        cp: Arc::new(ControlPlane::builder(kv, pubsub).build()),
        config: Arc::new(Config {
            providers: HashMap::from([(
                "novita".to_string(),
                ProviderConfig {
                    config: Some(proto::llm_provider_config::Config::OpenaiCompatible(
                        proto::GenericConfig {
                            name: "novita".to_string(),
                            base_url,
                            model: "test-model".to_string(),
                            api_key: Some(Secret {
                                source: Some(proto::secret::Source::Plain("test-key".to_string())),
                            }),
                        },
                    )),
                },
            )]),
            default_provider: "novita".to_string(),
            ..Config::default()
        }),
        mcp_registry: Arc::new(McpRegistry::new()),
        scheduler_authenticator: Arc::new(SchedulerRequestAuthenticator::deny_all()),
        worker_id: "test-worker".to_string(),
        fanout_hub: Arc::new(crate::worker::fanout::FanoutHub::new()),
        session_cancellations: Arc::new(
            crate::worker::session_control::SessionCancellationRegistry::default(),
        ),
    }
}

#[tokio::test]
async fn workflow_filter_agent_gates_lorem_generator_reply() {
    let _guard = crate::test_support::async_env_mutex().lock().await;
    let app = Router::new().route(
        "/chat/completions",
        post(|Json(body): Json<Value>| async move {
            let prompt = body
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|messages| messages.last())
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let connector_text = prompt
                .lines()
                .find_map(|line| line.strip_prefix("Connector message text: "))
                .unwrap_or_default();
            let content = if prompt.starts_with("Generate lorem ipsum for this request:") {
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".to_string()
            } else if connector_text.contains("lorem ipsum") {
                serde_json::to_string(&json!({
                    "execute": true,
                    "decision": "execute",
                    "silent": false,
                    "reason": "lorem ipsum requested"
                }))
                .expect("filter response should encode")
            } else {
                serde_json::to_string(&json!({
                    "execute": false,
                    "decision": "skip",
                    "silent": true,
                    "reason": "lorem ipsum not requested"
                }))
                .expect("filter response should encode")
            };
            let chunk = serde_json::json!({
                "choices": [{
                    "delta": { "content": content },
                    "index": 0
                }]
            });
            axum::response::Response::builder()
                .status(axum::http::StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from(format!(
                    "data: {}\n\ndata: [DONE]\n",
                    chunk
                )))
                .expect("mock LLM response should build")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake LLM should bind");
    let addr = listener.local_addr().expect("fake LLM should have addr");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("fake LLM should serve");
    });
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler =
        workflow_handler_with_base_url(kv.clone(), pubsub.clone(), format!("http://{addr}"));
    let workflow = ::control::control::manifest::parse_workflow(
        r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: lorem-router
  namespace: smoke
spec:
  steps:
    - id: filter
      type: agent
      agent: filter-agent
      prompt: |
        Decide whether this inbound connector message requested lorem ipsum.
        Connector message text: ${$.input.message.text}
      output:
        format: json
        schema:
          type: object
          required: [execute, decision, silent, reason]
          properties:
            execute:
              type: boolean
            decision:
              type: string
            silent:
              type: boolean
            reason:
              type: string
    - id: generate
      type: agent
      after: [filter]
      when:
        path: $.steps.filter.output.execute
        equals: true
      agent: lorem-generator
      prompt: |
        Generate lorem ipsum for this request:
        ${$.input.message.text}
  output:
    decision: ${$.steps.filter.output.decision}
    reason: ${$.steps.filter.output.reason}
    reply:
      silent: ${$.steps.filter.output.silent}
      text: "Generated after lorem filter: ${$.steps.generate.output.text}"
"#,
    )
    .expect("workflow YAML should parse");
    kv.set_msg(&keys::workflow("smoke", "lorem-router"), &workflow)
        .await
        .expect("workflow should persist");
    for agent in ["filter-agent", "lorem-generator"] {
        kv.set_msg(
            &keys::agent("smoke", agent),
            &agent_fixture(
                "smoke",
                agent,
                manifests::AgentSpec {
                    features: Vec::new(),
                    model_policy: None,
                    system_prompt: String::new(),
                    post_history_prompt: String::new(),
                    mcp_server_refs: Vec::new(),
                    capabilities: HashMap::new(),
                    a2a: None,
                    runtime: None,
                },
            ),
        )
        .await
        .expect("agent should persist");
    }

    let skipped_run = create_run(
        &handler.cp,
        &workflow,
        r#"{"message":{"text":"please summarize this account"}}"#.to_string(),
        HashMap::new(),
    )
    .await
    .expect("skip run should create");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&skipped_run, "created"))
        .await
        .expect("skip run should start filter");
    let filter_dispatch = latest_session_dispatch(&pubsub, "filter-agent").await;
    handler
        .handle_session_message(SessionDispatchEvent {
            direction: MessageDirection::Inbound as i32,
            ..filter_dispatch
        })
        .await
        .expect("filter session should complete");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&skipped_run, "child_session_completed"))
        .await
        .expect("skip run should complete");
    let completed_skip = stored_run(&kv, "smoke", "lorem-router", &skipped_run.id).await;
    assert_eq!(completed_skip.status, STATUS_COMPLETED);
    assert_eq!(
        stored_step(&kv, &completed_skip, "generate").await.status,
        STATUS_SKIPPED
    );
    assert_eq!(
        serde_json::from_str::<Value>(&completed_skip.output_json)
            .expect("skip output should decode"),
        json!({
            "decision": "skip",
            "reason": "lorem ipsum not requested",
            "reply": {
                "silent": true,
                "text": "Generated after lorem filter: "
            }
        })
    );

    let lorem_run = create_run(
        &handler.cp,
        &workflow,
        r#"{"message":{"text":"please generate lorem ipsum for the mock page"}}"#.to_string(),
        HashMap::new(),
    )
    .await
    .expect("lorem run should create");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&lorem_run, "created"))
        .await
        .expect("lorem run should start filter");
    let filter_dispatch = latest_session_dispatch(&pubsub, "filter-agent").await;
    handler
        .handle_session_message(SessionDispatchEvent {
            direction: MessageDirection::Inbound as i32,
            ..filter_dispatch
        })
        .await
        .expect("filter session should complete");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&lorem_run, "child_session_completed"))
        .await
        .expect("lorem run should start generator");
    let waiting = stored_run(&kv, "smoke", "lorem-router", &lorem_run.id).await;
    assert_eq!(waiting.status, STATUS_WAITING_CHILDREN);
    assert_eq!(
        stored_step(&kv, &waiting, "generate").await.status,
        STATUS_WAITING_CHILD_SESSION
    );
    let generator_dispatch = latest_session_dispatch(&pubsub, "lorem-generator").await;
    handler
        .handle_session_message(SessionDispatchEvent {
            direction: MessageDirection::Inbound as i32,
            ..generator_dispatch
        })
        .await
        .expect("generator session should complete");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&lorem_run, "child_session_completed"))
        .await
        .expect("lorem run should complete");
    let completed_lorem = stored_run(&kv, "smoke", "lorem-router", &lorem_run.id).await;
    assert_eq!(completed_lorem.status, STATUS_COMPLETED);
    assert_eq!(
        stored_step(&kv, &completed_lorem, "generate").await.status,
        STATUS_COMPLETED
    );
    assert_eq!(
        serde_json::from_str::<Value>(&completed_lorem.output_json)
            .expect("lorem output should decode"),
        json!({
            "decision": "execute",
            "reason": "lorem ipsum requested",
            "reply": {
                "silent": false,
                "text": "Generated after lorem filter: Lorem ipsum dolor sit amet, consectetur adipiscing elit."
            }
        })
    );

    server.abort();
}

#[tokio::test]
async fn child_workflow_step_waits_for_child_run_and_parent_redispatch_completes() {
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler = workflow_handler(kv.clone(), pubsub);
    let child = workflow_fixture(
        "default",
        "child-copy",
        resources_proto::WorkflowSpec {
            steps: vec![resources_proto::WorkflowStep {
                id: "copy".to_string(),
                r#type: "transform".to_string(),
                input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                ..Default::default()
            }],
            output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
            ..Default::default()
        },
    );
    let parent = workflow_fixture(
        "default",
        "parent",
        resources_proto::WorkflowSpec {
            steps: vec![resources_proto::WorkflowStep {
                id: "child".to_string(),
                r#type: "workflow".to_string(),
                workflow: "child-copy".to_string(),
                input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                ..Default::default()
            }],
            output_json: r#"{"answer":"${$.steps.child.output.answer}"}"#.to_string(),
            ..Default::default()
        },
    );
    kv.set_msg(&keys::workflow("default", "child-copy"), &child)
        .await
        .unwrap();
    kv.set_msg(&keys::workflow("default", "parent"), &parent)
        .await
        .unwrap();
    let parent_run = create_run(
        &handler.cp,
        &parent,
        r#"{"answer":"from-child"}"#.to_string(),
        HashMap::new(),
    )
    .await
    .unwrap();

    handler
        .handle_workflow_dispatch(workflow_dispatch(&parent_run, "created"))
        .await
        .unwrap();
    let waiting_parent = stored_run(&kv, "default", "parent", &parent_run.id).await;
    assert_eq!(waiting_parent.status, STATUS_WAITING_CHILDREN);
    let child_step = stored_step(&kv, &waiting_parent, "child").await;
    assert_eq!(child_step.status, STATUS_WAITING_CHILD_WORKFLOW);

    let child_run = stored_run(
        &kv,
        "default",
        "child-copy",
        &child_step.child_workflow_run_id,
    )
    .await;
    handler
        .handle_workflow_dispatch(workflow_dispatch(&child_run, "created"))
        .await
        .unwrap();
    assert_eq!(
        stored_run(&kv, "default", "child-copy", &child_run.id)
            .await
            .status,
        STATUS_COMPLETED
    );

    handler
        .handle_workflow_dispatch(workflow_dispatch(&parent_run, "child_completed"))
        .await
        .unwrap();
    let completed_parent = stored_run(&kv, "default", "parent", &parent_run.id).await;
    assert_eq!(completed_parent.status, STATUS_COMPLETED);
    assert_eq!(
        serde_json::from_str::<Value>(&completed_parent.output_json).unwrap(),
        json!({ "answer": "from-child" })
    );
}

#[tokio::test]
async fn redispatch_terminal_run_is_idempotent_and_cancelled_run_is_not_claimed() {
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler = workflow_handler(kv.clone(), pubsub);
    let workflow = workflow_fixture(
        "default",
        "once",
        resources_proto::WorkflowSpec {
            steps: vec![resources_proto::WorkflowStep {
                id: "copy".to_string(),
                r#type: "transform".to_string(),
                input_json: r#"{"answer":"${$.input.answer}"}"#.to_string(),
                ..Default::default()
            }],
            output_json: r#"{"answer":"${$.steps.copy.output.answer}"}"#.to_string(),
            ..Default::default()
        },
    );
    kv.set_msg(&keys::workflow("default", "once"), &workflow)
        .await
        .unwrap();
    let run = create_run(
        &handler.cp,
        &workflow,
        r#"{"answer":"yes"}"#.to_string(),
        HashMap::new(),
    )
    .await
    .unwrap();

    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
        .await
        .unwrap();
    let completed = stored_run(&kv, "default", "once", &run.id).await;
    assert_eq!(completed.status, STATUS_COMPLETED);
    let completed_events =
        event_type_count(&kv, "default", "once", &run.id, "step_completed").await;

    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "duplicate"))
        .await
        .unwrap();
    assert_eq!(
        event_type_count(&kv, "default", "once", &run.id, "step_completed").await,
        completed_events
    );

    let cancel_run_model = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
        .await
        .unwrap();
    cancel_run(&handler.cp, "default", "once", &cancel_run_model.id)
        .await
        .unwrap();
    assert!(claim_run(
        handler.cp.kv.as_ref(),
        "default",
        "once",
        &cancel_run_model.id,
        Utc::now(),
        "test",
    )
    .await
    .unwrap()
    .is_none());
    assert_eq!(
        stored_run(&kv, "default", "once", &cancel_run_model.id)
            .await
            .status,
        STATUS_CANCELLED
    );
    assert!(
        workflow_event_types(&kv, "default", "once", &cancel_run_model.id)
            .await
            .iter()
            .any(|event| event == "run_cancelled")
    );

    let claimed_cancel_run = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
        .await
        .unwrap();
    let stale_claim = claim_run(
        handler.cp.kv.as_ref(),
        "default",
        "once",
        &claimed_cancel_run.id,
        Utc::now(),
        "test",
    )
    .await
    .unwrap()
    .expect("run should be claimable");
    cancel_run(&handler.cp, "default", "once", &claimed_cancel_run.id)
        .await
        .unwrap();
    advance_run(&handler.cp, stale_claim).await.unwrap();

    let stored_cancelled = stored_run(&kv, "default", "once", &claimed_cancel_run.id).await;
    assert_eq!(stored_cancelled.status, STATUS_CANCELLED);
    let event_types = workflow_event_types(&kv, "default", "once", &claimed_cancel_run.id).await;
    assert!(event_types.iter().any(|event| event == "run_cancelled"));
    assert!(!event_types.iter().any(|event| event == "run_completed"));
}

#[tokio::test]
async fn wait_step_suspends_and_resumes_with_resume_payload_available_downstream() {
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler = workflow_handler(kv.clone(), pubsub);
    let workflow = workflow_fixture(
            "default",
            "waiter",
            resources_proto::WorkflowSpec {
                steps: vec![
                    resources_proto::WorkflowStep {
                        id: "externalSignal".to_string(),
                        r#type: "wait".to_string(),
                        prompt: "Wait for signal".to_string(),
                        resume_schema_json:
                            r#"{"type":"object","required":["token"],"properties":{"token":{"type":"string"}}}"#
                                .to_string(),
                        ..Default::default()
                    },
                    resources_proto::WorkflowStep {
                        id: "afterWait".to_string(),
                        r#type: "transform".to_string(),
                        after: vec!["externalSignal".to_string()],
                        input_json: r#"{"token":"${$.steps.externalSignal.resume.token}"}"#.to_string(),
                        ..Default::default()
                    },
                ],
                output_json: r#"{"token":"${$.steps.afterWait.output.token}"}"#.to_string(),
                ..Default::default()
            },
        );
    kv.set_msg(&keys::workflow("default", "waiter"), &workflow)
        .await
        .unwrap();
    let run = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
        .await
        .unwrap();

    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
        .await
        .unwrap();
    let suspended = stored_run(&kv, "default", "waiter", &run.id).await;
    assert_eq!(suspended.status, STATUS_SUSPENDED);
    assert_eq!(
        stored_step(&kv, &suspended, "externalSignal").await.status,
        STATUS_SUSPENDED
    );

    resume_run(
        &handler.cp,
        "default",
        "waiter",
        &run.id,
        "externalSignal",
        r#"{"token":"ready"}"#,
    )
    .await
    .unwrap();
    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "resumed"))
        .await
        .unwrap();

    let completed = stored_run(&kv, "default", "waiter", &run.id).await;
    assert_eq!(completed.status, STATUS_COMPLETED);
    assert_eq!(
        serde_json::from_str::<Value>(&completed.output_json).unwrap(),
        json!({ "token": "ready" })
    );
}

#[tokio::test]
async fn complex_workflow_yaml_runs_end_to_end_with_mock_llm_agent_and_resume() {
    let _guard = crate::test_support::async_env_mutex().lock().await;
    let app = Router::new().route(
            "/chat/completions",
            post(|| async {
                axum::response::Response::builder()
                    .status(axum::http::StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"mock retention action from LLM\"}}]}\n\n",
                        "data: [DONE]\n"
                    )))
                    .expect("mock LLM response should build")
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fake LLM should bind");
    let addr = listener.local_addr().expect("fake LLM should have addr");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("fake LLM should serve");
    });
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler =
        workflow_handler_with_base_url(kv.clone(), pubsub.clone(), format!("http://{addr}"));

    let workflow = ::control::control::manifest::parse_workflow(
            r#"
apiVersion: talon.impalasys.com/v1
kind: Workflow
metadata:
  name: complex-retention
  namespace: customer-retention
  labels:
    app: retention
spec:
  description: Complex retention workflow with fanout, branching, pause, and agent execution.
  inputSchema:
    type: object
    required: [accountId, risk, urgency]
    properties:
      accountId:
        type: string
      risk:
        type: string
      urgency:
        type: string
  outputSchema:
    type: object
    required: [summary, action, approved]
    properties:
      summary:
        type: string
      action:
        type: string
      approved:
        type: boolean
  steps:
    - id: intake
      type: transform
      input:
        accountId: ${$.input.accountId}
        risk: ${$.input.risk}
        summary: Account ${$.input.accountId} is ${$.input.risk} risk

    - id: profile
      type: transform
      after: [intake]
      input:
        profileSummary: Profile for ${$.steps.intake.output.accountId}

    - id: policy
      type: transform
      after: [intake]
      input:
        policySummary: Policy for ${$.input.urgency}

    - id: lowTouch
      type: transform
      after: [intake]
      when:
        path: $.steps.intake.output.risk
        notEquals: high
      input:
        action: low-touch

    - id: approval
      type: pause
      after: [intake]
      when:
        path: $.steps.intake.output.risk
        equals: high
      prompt: Approve retention action for ${$.steps.intake.output.accountId}?
      resumeSchema:
        type: object
        required: [approved]
        properties:
          approved:
            type: boolean

    - id: merge
      type: transform
      after: [profile, policy]
      input:
        summary: ${$.steps.intake.output.summary}; ${$.steps.profile.output.profileSummary}; ${$.steps.policy.output.policySummary}

    - id: draftAction
      type: agent
      after: [merge, approval, lowTouch]
      when:
        any:
          - path: $.steps.intake.output.risk
            notEquals: high
          - path: $.steps.approval.resume.approved
            equals: true
      agent: campaign-writer
      prompt: |
        Draft a retention action.
        ${$.steps.merge.output.summary}

    - id: final
      type: transform
      after: [draftAction]
      input:
        action: ${$.steps.draftAction.output.text}
        approved: ${$.steps.approval.resume.approved}
  output:
    summary: ${$.steps.merge.output.summary}
    action: ${$.steps.final.output.action}
    approved: ${$.steps.final.output.approved}
"#,
        )
        .expect("complex workflow YAML should parse");
    kv.set_msg(
        &keys::workflow("customer-retention", "complex-retention"),
        &workflow,
    )
    .await
    .expect("workflow should persist");
    kv.set_msg(
        &keys::agent("customer-retention", "campaign-writer"),
        &agent_fixture(
            "customer-retention",
            "campaign-writer",
            manifests::AgentSpec {
                features: Vec::new(),
                model_policy: None,
                system_prompt: "Write concise retention actions.".to_string(),
                post_history_prompt: String::new(),
                mcp_server_refs: Vec::new(),
                capabilities: HashMap::new(),
                a2a: None,
                runtime: None,
            },
        ),
    )
    .await
    .expect("agent should persist");

    let run = create_run(
        &handler.cp,
        &workflow,
        r#"{"accountId":"acct_123","risk":"high","urgency":"normal"}"#.to_string(),
        HashMap::new(),
    )
    .await
    .expect("run should create");

    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
        .await
        .expect("first workflow dispatch should run until pause");
    let suspended = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
    assert_eq!(suspended.status, STATUS_SUSPENDED);
    assert_eq!(
        stored_step(&kv, &suspended, "intake").await.status,
        STATUS_COMPLETED
    );
    assert_eq!(
        stored_step(&kv, &suspended, "profile").await.status,
        STATUS_COMPLETED
    );
    assert_eq!(
        stored_step(&kv, &suspended, "policy").await.status,
        STATUS_COMPLETED
    );
    assert_eq!(
        stored_step(&kv, &suspended, "merge").await.status,
        STATUS_COMPLETED
    );
    assert_eq!(
        stored_step(&kv, &suspended, "lowTouch").await.status,
        STATUS_SKIPPED
    );
    assert_eq!(
        stored_step(&kv, &suspended, "approval").await.status,
        STATUS_SUSPENDED
    );
    assert!(resume_run(
        &handler.cp,
        "customer-retention",
        "complex-retention",
        &run.id,
        "approval",
        r#"{"approved":"yes"}"#,
    )
    .await
    .expect_err("invalid resume payload should be rejected")
    .to_string()
    .contains("resume.approved must be boolean"));

    resume_run(
        &handler.cp,
        "customer-retention",
        "complex-retention",
        &run.id,
        "approval",
        r#"{"approved":true}"#,
    )
    .await
    .expect("valid resume should be accepted");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "resumed"))
        .await
        .expect("resumed workflow should start child agent");
    let waiting = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
    assert_eq!(waiting.status, STATUS_WAITING_CHILDREN);
    let draft_step = stored_step(&kv, &waiting, "draftAction").await;
    assert_eq!(draft_step.status, STATUS_WAITING_CHILD_SESSION);
    let session_id = draft_step.child_session_id.clone();
    let session = kv
        .get_msg::<data_proto::Session>(&keys::session(
            "customer-retention",
            "campaign-writer",
            &session_id,
        ))
        .await
        .expect("child session should load")
        .expect("child session should exist");
    assert_eq!(
        session.labels.get(LABEL_WORKFLOW),
        Some(&"complex-retention".to_string())
    );
    assert_eq!(session.labels.get(LABEL_WORKFLOW_RUN), Some(&run.id));
    assert_eq!(
        session.labels.get(LABEL_WORKFLOW_STEP),
        Some(&"draftAction".to_string())
    );

    let child_event = latest_session_dispatch(&pubsub, "campaign-writer").await;
    handler
        .handle_session_message(SessionDispatchEvent {
            direction: MessageDirection::Inbound as i32,
            ..child_event
        })
        .await
        .expect("mock LLM child session should complete");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "child_session_completed"))
        .await
        .expect("parent workflow should complete after child session");

    let completed = stored_run(&kv, "customer-retention", "complex-retention", &run.id).await;
    assert_eq!(completed.status, STATUS_COMPLETED);
    assert_eq!(
        serde_json::from_str::<Value>(&completed.output_json).expect("output should decode"),
        json!({
            "summary": "Account acct_123 is high risk; Profile for acct_123; Policy for normal",
            "action": "mock retention action from LLM",
            "approved": true
        })
    );
    assert_eq!(
        serde_json::from_str::<Value>(
            &stored_step(&kv, &completed, "draftAction")
                .await
                .output_json
        )
        .expect("agent output should decode"),
        json!({ "text": "mock retention action from LLM" })
    );
    let event_types =
        workflow_event_types(&kv, "customer-retention", "complex-retention", &run.id).await;
    assert!(event_types.iter().any(|event| event == "run_started"));
    assert!(event_types.iter().any(|event| event == "step_skipped"));
    assert!(event_types.iter().any(|event| event == "run_suspended"));
    assert!(event_types.iter().any(|event| event == "run_resumed"));
    assert!(event_types.iter().any(|event| event == "run_completed"));

    server.abort();
}

#[tokio::test]
async fn failed_child_agent_session_fails_step_without_using_stale_output() {
    let kv = Arc::new(MockKvStore::new());
    let pubsub = Arc::new(RecordingPubSub::default());
    let handler = workflow_handler(kv.clone(), pubsub);
    let workflow = workflow_fixture(
        "customer-retention",
        "agent-failure",
        resources_proto::WorkflowSpec {
            steps: vec![resources_proto::WorkflowStep {
                id: "draft".to_string(),
                r#type: "agent".to_string(),
                agent: "campaign-writer".to_string(),
                prompt: "Draft an action".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    kv.set_msg(
        &keys::workflow("customer-retention", "agent-failure"),
        &workflow,
    )
    .await
    .expect("workflow should persist");
    kv.set_msg(
        &keys::agent("customer-retention", "campaign-writer"),
        &agent_fixture(
            "customer-retention",
            "campaign-writer",
            manifests::AgentSpec {
                features: Vec::new(),
                model_policy: None,
                system_prompt: "Write concise retention actions.".to_string(),
                post_history_prompt: String::new(),
                mcp_server_refs: Vec::new(),
                capabilities: HashMap::new(),
                a2a: None,
                runtime: None,
            },
        ),
    )
    .await
    .expect("agent should persist");

    let run = create_run(&handler.cp, &workflow, "{}".to_string(), HashMap::new())
        .await
        .expect("run should create");
    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "created"))
        .await
        .expect("workflow should start child session");
    let waiting = stored_run(&kv, "customer-retention", "agent-failure", &run.id).await;
    let draft_step = stored_step(&kv, &waiting, "draft").await;
    assert_eq!(draft_step.status, STATUS_WAITING_CHILD_SESSION);

    let session_key = keys::session(
        "customer-retention",
        "campaign-writer",
        &draft_step.child_session_id,
    );
    let mut session = kv
        .get_msg::<data_proto::Session>(&session_key)
        .await
        .expect("session should load")
        .expect("session should exist");
    kv.set_msg(
        &keys::session_message(
            "customer-retention",
            "campaign-writer",
            &draft_step.child_session_id,
            "old-assistant",
        ),
        &data_proto::SessionMessage {
            id: "old-assistant".to_string(),
            role: data_proto::MessageRole::RoleAssistant as i32,
            created_at: 1,
            labels: HashMap::new(),
            parts: vec![data_proto::SessionMessagePart {
                id: "old-text".to_string(),
                part_type: data_proto::SessionMessagePartType::Text as i32,
                content: "stale output from previous turn".to_string(),
                name: String::new(),
                payload_json: String::new(),
                created_at: 1,
                object: None,
            }],
        },
    )
    .await
    .expect("stale assistant message should persist");
    session.status = "ERROR".to_string();
    kv.set_msg(&session_key, &session)
        .await
        .expect("failed child session should persist");

    handler
        .handle_workflow_dispatch(workflow_dispatch(&run, "child_session_completed"))
        .await
        .expect("workflow should process failed child session");
    let failed = stored_run(&kv, "customer-retention", "agent-failure", &run.id).await;
    let failed_step = stored_step(&kv, &failed, "draft").await;
    assert_eq!(failed_step.status, STATUS_FAILED);
    assert!(failed_step.error.contains("child session"));
    assert_ne!(
        failed_step.output_json,
        r#"{"text":"stale output from previous turn"}"#
    );
    assert_eq!(failed.status, STATUS_FAILED);
}

use ::control::control::keys;
use ::control::control::resource_model::TypedResource;
use ::control::control::ControlPlane;
use ::control::control::KeyValueStore;
use ::protocol::data_proto;
use ::protocol::resources_proto;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;
