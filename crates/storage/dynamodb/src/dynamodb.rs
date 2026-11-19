// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[allow(unused_imports)]
use crate::talon;

use talon::contracts::keys::ResourceKey;

use talon::contracts::keys::ResourceList;

use talon::contracts::KeyValueStore;

use talon::contracts::ListOptions;

use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::{
    config::{AsyncSleep, Credentials},
    primitives::Blob,
    types::AttributeValue,
    Client,
};
use std::collections::HashMap;
use talon::contracts::Order;

// DynamoDB table keys. `PK` encodes the Talon namespace/resource parent, while
// `SK` encodes the direct child's kind/name. Together they are enough to
// reconstruct the ResourceKey returned by list operations.
const PK_ATTR: &str = "PK";
const SK_ATTR: &str = "SK";

// Serialized protobuf payload for the resource.
const VALUE_ATTR: &str = "Value";

fn conditional_cancellation(reasons: &[aws_sdk_dynamodb::types::CancellationReason]) -> bool {
    reasons
        .iter()
        .any(|reason| reason.code() == Some("ConditionalCheckFailed"))
        && reasons.iter().all(|reason| {
            matches!(
                reason.code(),
                None | Some("None" | "ConditionalCheckFailed")
            )
        })
}
fn transaction_conflict(reasons: &[aws_sdk_dynamodb::types::CancellationReason]) -> bool {
    reasons
        .iter()
        .any(|reason| reason.code() == Some("TransactionConflict"))
        && reasons
            .iter()
            .all(|reason| matches!(reason.code(), None | Some("None" | "TransactionConflict")))
}

#[derive(Clone)]
pub struct DynamoDbKvStore {
    client: Client,
    table: String,
}

impl DynamoDbKvStore {
    pub fn new(client: Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    pub async fn from_env(table: impl Into<String>) -> Result<Self> {
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(RegionProviderChain::default_provider().or_else("us-east-1"));
        if let Ok(endpoint_url) = std::env::var("TALON_DYNAMODB_ENDPOINT_URL") {
            if !endpoint_url.trim().is_empty() {
                loader = loader
                    .credentials_provider(Credentials::new("fake", "fake", None, None, "local"))
                    .endpoint_url(endpoint_url);
            }
        }
        let config = loader.load().await;
        Ok(Self::new(Client::new(&config), table))
    }

    fn key_item(&self, key: &ResourceKey) -> HashMap<String, AttributeValue> {
        HashMap::from([
            (PK_ATTR.to_string(), AttributeValue::S(pk_for_key(key))),
            (SK_ATTR.to_string(), AttributeValue::S(sk_for_key(key))),
        ])
    }

    fn put_item(&self, key: &ResourceKey, value: &[u8]) -> HashMap<String, AttributeValue> {
        let mut item = self.key_item(key);
        item.insert(
            VALUE_ATTR.to_string(),
            AttributeValue::B(Blob::new(value.to_vec())),
        );
        item
    }

    async fn query_list(
        &self,
        list: &ResourceList,
        options: ListOptions<'_>,
        include_values: bool,
    ) -> Result<Vec<(ResourceKey, Option<Vec<u8>>)>> {
        if options.limit == Some(0) {
            return Ok(Vec::new());
        }

        let pk = pk_for_list(list);
        let mut names = HashMap::from([("#pk".to_string(), PK_ATTR.to_string())]);
        let mut values = HashMap::from([(":pk".to_string(), AttributeValue::S(pk))]);
        let mut key_condition = "#pk = :pk".to_string();
        if let Some(kind) = list.kind.as_deref() {
            names.insert("#sk".to_string(), SK_ATTR.to_string());
            values.insert(
                ":sk_prefix".to_string(),
                AttributeValue::S(sk_prefix_for_kind(kind)),
            );
            key_condition = "#pk = :pk AND begins_with(#sk, :sk_prefix)".to_string();
        }
        let mut start_key = None;
        let mut rows = Vec::new();
        loop {
            let remaining_limit = options
                .limit
                .and_then(|limit| limit.checked_sub(rows.len()))
                .unwrap_or(usize::MAX);
            if remaining_limit == 0 {
                break;
            }

            let page_limit = if options.limit.is_some() {
                Some(i32::try_from(remaining_limit.min(1000))?)
            } else {
                None
            };
            let mut request = self
                .client
                .query()
                .table_name(&self.table)
                .consistent_read(true)
                .scan_index_forward(options.order != Order::Desc)
                .key_condition_expression(key_condition.clone())
                .set_expression_attribute_names(Some(names.clone()))
                .set_expression_attribute_values(Some(values.clone()))
                .set_exclusive_start_key(start_key);
            if let Some(page_limit) = page_limit {
                request = request.limit(page_limit);
            }

            let output = request.send().await?;
            for item in output.items() {
                let key = key_from_item(item)?;
                if options
                    .before_name
                    .is_some_and(|before| key.name.as_str() >= before)
                {
                    continue;
                }
                if options
                    .after_name
                    .is_some_and(|after| key.name.as_str() <= after)
                {
                    continue;
                }
                let value = if include_values {
                    Some(value_from_item(item)?)
                } else {
                    None
                };
                rows.push((key, value));
                if options.limit.is_some_and(|limit| rows.len() >= limit) {
                    return Ok(rows);
                }
            }

            start_key = output.last_evaluated_key().cloned();
            if start_key.is_none() {
                break;
            }
        }

        Ok(rows)
    }
}

#[async_trait::async_trait]
impl KeyValueStore for DynamoDbKvStore {
    async fn get(&self, key: &ResourceKey) -> Result<Option<Vec<u8>>> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table)
            .set_key(Some(self.key_item(key)))
            .consistent_read(true)
            .send()
            .await?;
        output.item().map(value_from_item).transpose()
    }

    async fn set(&self, key: &ResourceKey, value: &[u8]) -> Result<()> {
        self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(self.put_item(key, value)))
            .send()
            .await?;
        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &ResourceKey,
        expected: Option<&[u8]>,
        value: &[u8],
    ) -> Result<bool> {
        let mut names = HashMap::new();
        let mut values = HashMap::new();
        let condition = if let Some(expected) = expected {
            names.insert("#value".to_string(), VALUE_ATTR.to_string());
            values.insert(
                ":expected".to_string(),
                AttributeValue::B(Blob::new(expected.to_vec())),
            );
            "#value = :expected"
        } else {
            names.insert("#pk".to_string(), PK_ATTR.to_string());
            "attribute_not_exists(#pk)"
        };

        let result = self
            .client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(self.put_item(key, value)))
            .condition_expression(condition)
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values((!values.is_empty()).then_some(values))
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(err)
                if err
                    .as_service_error()
                    .is_some_and(|err| err.is_conditional_check_failed_exception()) =>
            {
                Ok(false)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn compare_and_swap_batch(
        &self,
        operations: &[talon::contracts::KeyValueBatchOperation],
    ) -> Result<bool> {
        use aws_sdk_dynamodb::types::{ConditionCheck, Delete, Put, TransactWriteItem};
        talon::contracts::validate_key_value_batch(operations)?;
        if operations.is_empty() {
            return Ok(true);
        }
        let mut items = Vec::with_capacity(operations.len());
        for operation in operations {
            let (condition, names, values) = if let Some(expected) = operation.expected.as_deref() {
                (
                    "#v = :e",
                    HashMap::from([("#v".into(), VALUE_ATTR.into())]),
                    Some(HashMap::from([(
                        ":e".into(),
                        AttributeValue::B(Blob::new(expected.to_vec())),
                    )])),
                )
            } else {
                (
                    "attribute_not_exists(#pk)",
                    HashMap::from([("#pk".into(), PK_ATTR.into())]),
                    None,
                )
            };
            let item = if let Some(value) = operation.value.as_deref() {
                TransactWriteItem::builder()
                    .put(
                        Put::builder()
                            .table_name(&self.table)
                            .set_item(Some(self.put_item(&operation.key, value)))
                            .condition_expression(condition)
                            .set_expression_attribute_names(Some(names))
                            .set_expression_attribute_values(values)
                            .build()?,
                    )
                    .build()
            } else if operation.expected.is_some() {
                TransactWriteItem::builder()
                    .delete(
                        Delete::builder()
                            .table_name(&self.table)
                            .set_key(Some(self.key_item(&operation.key)))
                            .condition_expression(condition)
                            .set_expression_attribute_names(Some(names))
                            .set_expression_attribute_values(values)
                            .build()?,
                    )
                    .build()
            } else {
                TransactWriteItem::builder()
                    .condition_check(
                        ConditionCheck::builder()
                            .table_name(&self.table)
                            .set_key(Some(self.key_item(&operation.key)))
                            .condition_expression(condition)
                            .set_expression_attribute_names(Some(names))
                            .build()?,
                    )
                    .build()
            };
            items.push(item);
        }
        for attempt in 0..5 {
            match self
                .client
                .transact_write_items()
                .set_transact_items(Some(items.clone()))
                .client_request_token(talon::contracts::uuid::resource_version())
                .send()
                .await
            {
                Ok(_) => return Ok(true),
                Err(error) => {
                    if let Some(TransactWriteItemsError::TransactionCanceledException(canceled)) =
                        error.as_service_error()
                    {
                        if conditional_cancellation(canceled.cancellation_reasons()) {
                            return Ok(false);
                        }
                        if attempt < 4 && transaction_conflict(canceled.cancellation_reasons()) {
                            if let Some(sleep) = self.client.config().sleep_impl() {
                                sleep
                                    .sleep(std::time::Duration::from_millis(5 << attempt))
                                    .await;
                                continue;
                            }
                        }
                    }
                    return Err(error.into());
                }
            }
        }
        unreachable!("each transaction attempt returns or retries")
    }

    async fn compare_and_delete(&self, key: &ResourceKey, expected: &[u8]) -> Result<bool> {
        let result = self
            .client
            .delete_item()
            .table_name(&self.table)
            .set_key(Some(self.key_item(key)))
            .condition_expression("#value = :expected")
            .expression_attribute_names("#value", VALUE_ATTR)
            .expression_attribute_values(
                ":expected",
                AttributeValue::B(Blob::new(expected.to_vec())),
            )
            .send()
            .await;
        match result {
            Ok(_) => Ok(true),
            Err(err)
                if err
                    .as_service_error()
                    .is_some_and(|err| err.is_conditional_check_failed_exception()) =>
            {
                Ok(false)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn delete(&self, key: &ResourceKey) -> Result<()> {
        self.client
            .delete_item()
            .table_name(&self.table)
            .set_key(Some(self.key_item(key)))
            .send()
            .await?;
        Ok(())
    }

    async fn list_keys(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> Result<Vec<ResourceKey>> {
        self.query_list(list, options.unwrap_or_default(), false)
            .await
            .map(|rows| rows.into_iter().map(|(key, _)| key).collect())
    }

    async fn list_entries(
        &self,
        list: &ResourceList,
        options: Option<ListOptions<'_>>,
    ) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        self.query_list(list, options.unwrap_or_default(), true)
            .await
            .map(|rows| {
                rows.into_iter()
                    .filter_map(|(key, value)| value.map(|value| (key, value)))
                    .collect()
            })
    }
}

fn pk_for_key(key: &ResourceKey) -> String {
    pk_for_parent(&key.namespace, &key.parent_path)
}

fn pk_for_list(list: &ResourceList) -> String {
    pk_for_parent(&list.parent.namespace, &list.parent.parent_path)
}

fn pk_for_parent(namespace: &str, parent_path: &str) -> String {
    format!(
        "Namespace/{}/Resource/{}",
        encode_part(namespace),
        parent_path
    )
}

fn sk_for_key(key: &ResourceKey) -> String {
    sk_for_kind_name(&key.kind, &key.name)
}

fn sk_for_kind_name(kind: &str, name: &str) -> String {
    format!("{kind}/{name}")
}

fn sk_prefix_for_kind(kind: &str) -> String {
    format!("{kind}/")
}

fn encode_part(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn key_from_item(item: &HashMap<String, AttributeValue>) -> Result<ResourceKey> {
    let pk = string_attr(item, PK_ATTR)?;
    let sk = string_attr(item, SK_ATTR)?;
    resource_key_from_pk_sk(pk, sk)
}

fn resource_key_from_pk_sk(pk: &str, sk: &str) -> Result<ResourceKey> {
    let rest = pk
        .strip_prefix("Namespace/")
        .ok_or_else(|| anyhow!("DynamoDB PK does not start with Namespace/: {pk}"))?;
    let (namespace, parent_path) = rest
        .split_once("/Resource/")
        .ok_or_else(|| anyhow!("DynamoDB PK is missing /Resource/ separator: {pk}"))?;
    let (kind, name) = sk
        .split_once('/')
        .ok_or_else(|| anyhow!("DynamoDB SK must contain kind/name: {sk}"))?;
    Ok(ResourceKey {
        namespace: decode_part(namespace)?,
        parent_path: parent_path.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
    })
}

fn string_attr<'a>(item: &'a HashMap<String, AttributeValue>, attr: &str) -> Result<&'a str> {
    item.get(attr)
        .ok_or_else(|| anyhow!("DynamoDB item missing {attr} attribute"))?
        .as_s()
        .map(String::as_str)
        .map_err(|_| anyhow!("DynamoDB item attribute {attr} was not a string"))
}

fn value_from_item(item: &HashMap<String, AttributeValue>) -> Result<Vec<u8>> {
    Ok(item
        .get(VALUE_ATTR)
        .ok_or_else(|| anyhow!("DynamoDB item missing {VALUE_ATTR} attribute"))?
        .as_b()
        .map_err(|_| anyhow!("DynamoDB item attribute {VALUE_ATTR} was not binary"))?
        .as_ref()
        .to_vec())
}

fn decode_part(value: &str) -> Result<String> {
    urlencoding::decode(value)
        .map(|value| value.into_owned())
        .map_err(|err| anyhow!("failed to decode DynamoDB key segment '{value}': {err}"))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use crate::talon;

    use super::*;

    #[test]
    fn cancellation_errors_are_not_misreported_as_condition_conflicts() {
        use aws_sdk_dynamodb::types::CancellationReason;
        let reason = |code: &str| CancellationReason::builder().code(code).build();
        assert!(conditional_cancellation(&[
            reason("None"),
            reason("ConditionalCheckFailed")
        ]));
        for code in [
            "TransactionConflict",
            "ThrottlingError",
            "ProvisionedThroughputExceeded",
            "ValidationError",
            "ItemCollectionSizeLimitExceeded",
        ] {
            assert!(!conditional_cancellation(&[reason(code)]));
            assert!(!conditional_cancellation(&[
                reason("ConditionalCheckFailed"),
                reason(code)
            ]));
        }
        assert!(!conditional_cancellation(&[]));
        assert!(transaction_conflict(&[
            reason("None"),
            reason("TransactionConflict")
        ]));
    }

    #[test]
    fn dynamodb_local_atomic_batch_conformance() {
        talon::test_support::run_async_test(async {
            use aws_sdk_dynamodb::types::{
                AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
            };
            use std::process::Command;
            let _guard = talon::test_support::docker_test_guard();
            struct Container(String);
            impl Drop for Container {
                fn drop(&mut self) {
                    let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
                }
            }
            let name = talon::contracts::uuid::unique_name("talon-ddb");
            let run = Command::new("docker")
                .args([
                    "run",
                    "-d",
                    "--rm",
                    "--name",
                    &name,
                    "-p",
                    "127.0.0.1::8000",
                    "amazon/dynamodb-local:2.6.1",
                    "-jar",
                    "DynamoDBLocal.jar",
                    "-inMemory",
                    "-sharedDb",
                ])
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
            let _container = Container(name.clone());
            let inspect = Command::new("docker")
                .args([
                    "inspect",
                    "-f",
                    "{{(index (index .NetworkSettings.Ports \"8000/tcp\") 0).HostPort}}",
                    &name,
                ])
                .output()
                .unwrap();
            assert!(inspect.status.success());
            let port = String::from_utf8(inspect.stdout).unwrap();
            let config = aws_sdk_dynamodb::config::Builder::new()
                .behavior_version(aws_config::BehaviorVersion::latest())
                .region(aws_sdk_dynamodb::config::Region::new("us-east-1"))
                .credentials_provider(Credentials::new("local", "local", None, None, "test"))
                .endpoint_url(format!("http://127.0.0.1:{}", port.trim()))
                .build();
            let client = Client::from_conf(config);
            let mut ready = false;
            for _ in 0..60 {
                if client.list_tables().send().await.is_ok() {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            assert!(ready, "DynamoDB Local did not become ready");
            client
                .create_table()
                .table_name("atomic_test")
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name(PK_ATTR)
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name(SK_ATTR)
                        .attribute_type(ScalarAttributeType::S)
                        .build()
                        .unwrap(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name(PK_ATTR)
                        .key_type(KeyType::Hash)
                        .build()
                        .unwrap(),
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name(SK_ATTR)
                        .key_type(KeyType::Range)
                        .build()
                        .unwrap(),
                )
                .billing_mode(BillingMode::PayPerRequest)
                .send()
                .await
                .unwrap();
            let store = DynamoDbKvStore::new(client, "atomic_test");
            talon::test_support::assert_atomic_batch_conformance(&store).await;
        });
    }

    #[test]
    fn session_keys_project_resource_parent_into_partition_key() {
        let key = talon::contracts::keys::session("acme:team", "agent-1", "session-1");

        assert_eq!(
            pk_for_key(&key),
            "Namespace/acme%3Ateam/Resource/Agent/agent-1"
        );
        assert_eq!(sk_for_key(&key), "Session/session-1");
    }

    #[test]
    fn journal_keys_group_under_submission_parent() {
        let key = talon::contracts::keys::session_journal_entry(
            "acme:team",
            "agent-1",
            "session-1",
            "submission-1",
            "000001",
        );

        assert_eq!(
            pk_for_key(&key),
            "Namespace/acme%3Ateam/Resource/Agent/agent-1/Session/session-1/SessionSubmission/submission-1"
        );
        assert_eq!(sk_for_key(&key), "SessionJournalEntry/000001");
    }

    #[test]
    fn sort_keys_preserve_raw_resource_name_order() {
        let dash = sk_for_kind_name("Thing", "-");
        let colon = sk_for_kind_name("Thing", ":");

        assert!(dash < colon);
        assert_eq!(colon, "Thing/:");
    }

    #[test]
    fn resource_key_round_trips_through_pk_and_sk() {
        let key = talon::contracts::keys::session("acme:team", "agent-1", "session-1");

        assert_eq!(
            resource_key_from_pk_sk(&pk_for_key(&key), &sk_for_key(&key)).unwrap(),
            key
        );
    }
}
