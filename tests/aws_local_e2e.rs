// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#![cfg(feature = "aws-local-e2e")]

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::config::Credentials as DynamoDbCredentials;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
};
use aws_sdk_dynamodb::Client as DynamoDbClient;
use base64::{engine::general_purpose, Engine as _};
use talon::control::{
    keys,
    kv::DynamoDbKvStore,
    pubsub::{SqsMessagePublisher, TALON_TOPIC_ATTRIBUTE},
    KeyValueStore, MessagePublisher,
};
use testcontainers_modules::{
    dynamodb_local::DynamoDb,
    localstack::LocalStack,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

#[tokio::test]
#[ignore = "requires Docker and pulls amazon/dynamodb-local"]
async fn dynamodb_state_store_round_trips_and_compares() -> anyhow::Result<()> {
    let _env = env_lock().lock().expect("env lock poisoned");
    let node = DynamoDb::default().start().await?;
    let endpoint = endpoint_for(&node, 8000).await?;
    let client = dynamodb_local_client(&endpoint).await;
    ensure_dynamodb_table(client.clone(), "talon_state_e2e").await?;
    let store = DynamoDbKvStore::new(client, "talon_state_e2e");

    let key = keys::session("acme:team", "agent-1", "session-1");
    assert!(store.compare_and_swap(&key, None, b"one").await?);
    assert_eq!(store.get(&key).await?.as_deref(), Some(&b"one"[..]));
    assert!(!store.compare_and_swap(&key, None, b"two").await?);
    assert!(!store.compare_and_swap(&key, Some(b"wrong"), b"two").await?);
    assert!(store.compare_and_swap(&key, Some(b"one"), b"two").await?);
    assert_eq!(store.get(&key).await?.as_deref(), Some(&b"two"[..]));

    let keys = store
        .list_keys(&keys::session_prefix("acme:team", "agent-1"))
        .await?;
    assert_eq!(keys, vec![key]);
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker and pulls localstack/localstack"]
async fn sqs_pubsub_delivers_published_message() -> anyhow::Result<()> {
    let _env = env_lock().lock().expect("env lock poisoned");
    let node = LocalStack::default()
        .with_env_var("SERVICES", "sqs")
        .start()
        .await?;
    let endpoint = endpoint_for(&node, 4566).await?;
    let _endpoint = EnvGuard::set("TALON_SQS_ENDPOINT_URL", &endpoint);
    let _queue = EnvGuard::set("TALON_SQS_QUEUE_NAME", "talon-e2e");
    let publisher = SqsMessagePublisher::from_env().await?;
    let queue_url = publisher.queue_url().await?;

    publisher
        .publish("talon.session.dispatch", b"dispatch-payload")
        .await?;
    let response = tokio::time::timeout(
        Duration::from_secs(20),
        publisher
            .client()
            .receive_message()
            .queue_url(&queue_url)
            .max_number_of_messages(1)
            .message_attribute_names(TALON_TOPIC_ATTRIBUTE)
            .wait_time_seconds(10)
            .send(),
    )
    .await??;

    let message = response
        .messages()
        .first()
        .ok_or_else(|| anyhow::anyhow!("SQS did not deliver the published test message"))?;
    let body = message
        .body()
        .ok_or_else(|| anyhow::anyhow!("SQS delivered a message without a body"))?;
    let topic = message
        .message_attributes()
        .and_then(|attributes| attributes.get(TALON_TOPIC_ATTRIBUTE))
        .and_then(|attribute| attribute.string_value())
        .ok_or_else(|| anyhow::anyhow!("SQS delivered a message without Talon topic metadata"))?;
    assert_eq!(topic, "talon.session.dispatch");
    let delivered = general_purpose::STANDARD.decode(body)?;
    assert_eq!(delivered, b"dispatch-payload");

    if let Some(receipt_handle) = message.receipt_handle() {
        publisher
            .client()
            .delete_message()
            .queue_url(queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await?;
    }
    Ok(())
}

async fn dynamodb_local_client(endpoint: &str) -> DynamoDbClient {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(RegionProviderChain::default_provider().or_else("us-east-1"))
        .credentials_provider(DynamoDbCredentials::new(
            "fake", "fake", None, None, "local",
        ))
        .endpoint_url(endpoint)
        .load()
        .await;
    DynamoDbClient::new(&config)
}

async fn ensure_dynamodb_table(client: DynamoDbClient, table: &str) -> anyhow::Result<()> {
    match client.describe_table().table_name(table).send().await {
        Ok(output) => {
            let status = output
                .table()
                .and_then(|table| table.table_status())
                .map(|status| status.as_str());
            if status == Some("ACTIVE") {
                return Ok(());
            }
            return wait_until_dynamodb_table_active(&client, table).await;
        }
        Err(err)
            if err
                .as_service_error()
                .is_some_and(|err| err.is_resource_not_found_exception()) => {}
        Err(err) => return Err(err.into()),
    }

    let create_result = client
        .create_table()
        .table_name(table)
        .billing_mode(BillingMode::PayPerRequest)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("PK")
                .attribute_type(ScalarAttributeType::S)
                .build()?,
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("SK")
                .attribute_type(ScalarAttributeType::S)
                .build()?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("PK")
                .key_type(KeyType::Hash)
                .build()?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("SK")
                .key_type(KeyType::Range)
                .build()?,
        )
        .send()
        .await;
    match create_result {
        Ok(_) => {}
        Err(err)
            if err
                .as_service_error()
                .is_some_and(|err| err.is_resource_in_use_exception()) => {}
        Err(err) => return Err(err.into()),
    }

    wait_until_dynamodb_table_active(&client, table).await
}

async fn wait_until_dynamodb_table_active(
    client: &DynamoDbClient,
    table: &str,
) -> anyhow::Result<()> {
    for _ in 0..60 {
        let output = match client.describe_table().table_name(table).send().await {
            Ok(output) => output,
            Err(err) => {
                tracing::warn!(
                    table,
                    error = %err,
                    "Transient error describing DynamoDB table while waiting for ACTIVE status"
                );
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let status = output
            .table()
            .and_then(|table| table.table_status())
            .map(|status| status.as_str());
        if status == Some("ACTIVE") {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    anyhow::bail!("timed out waiting for DynamoDB table {table} to become ACTIVE")
}

async fn endpoint_for<I>(
    node: &testcontainers_modules::testcontainers::ContainerAsync<I>,
    port: u16,
) -> anyhow::Result<String>
where
    I: testcontainers_modules::testcontainers::Image,
{
    let host = node.get_host().await?;
    let port = node.get_host_port_ipv4(port).await?;
    Ok(format!("http://{host}:{port}"))
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}
