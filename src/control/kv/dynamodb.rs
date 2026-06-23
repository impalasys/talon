// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{
    keys::{ResourceKey, ResourceList},
    KeyValueStore,
};
use anyhow::{anyhow, Result};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::{config::Credentials, primitives::Blob, types::AttributeValue, Client};
use std::collections::HashMap;

// DynamoDB table keys. `PK` encodes the Talon namespace/resource parent, while
// `SK` encodes the direct child's kind/name. Together they are enough to
// reconstruct the ResourceKey returned by list operations.
const PK_ATTR: &str = "PK";
const SK_ATTR: &str = "SK";

// Serialized protobuf payload for the resource.
const VALUE_ATTR: &str = "Value";

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
        before_name: Option<&str>,
        limit: Option<usize>,
        include_values: bool,
    ) -> Result<Vec<(ResourceKey, Option<Vec<u8>>)>> {
        if limit == Some(0) {
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
        let descending_page = before_name.is_some() || limit.is_some();

        if let (Some(kind), Some(before_name)) = (list.kind.as_deref(), before_name) {
            key_condition = "#pk = :pk AND #sk BETWEEN :sk_prefix AND :before_sk".to_string();
            values.insert(
                ":before_sk".to_string(),
                AttributeValue::S(sk_for_kind_name(kind, before_name)),
            );
        } else if before_name.is_some() {
            return Err(anyhow!(
                "dynamodb list pagination requires a resource kind when before_name is set"
            ));
        }

        let mut start_key = None;
        let mut rows = Vec::new();
        loop {
            let remaining_limit = limit
                .and_then(|limit| limit.checked_sub(rows.len()))
                .unwrap_or(usize::MAX);
            if remaining_limit == 0 {
                break;
            }

            let page_limit = if limit.is_some() {
                Some(i32::try_from(remaining_limit.min(1000))?)
            } else {
                None
            };
            let mut request = self
                .client
                .query()
                .table_name(&self.table)
                .consistent_read(true)
                .scan_index_forward(!descending_page)
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
                if before_name.is_some_and(|before| key.name.as_str() >= before) {
                    continue;
                }
                let value = if include_values {
                    Some(value_from_item(item)?)
                } else {
                    None
                };
                rows.push((key, value));
                if limit.is_some_and(|limit| rows.len() >= limit) {
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

    async fn delete(&self, key: &ResourceKey) -> Result<()> {
        self.client
            .delete_item()
            .table_name(&self.table)
            .set_key(Some(self.key_item(key)))
            .send()
            .await?;
        Ok(())
    }

    async fn list_keys(&self, list: &ResourceList) -> Result<Vec<ResourceKey>> {
        self.query_list(list, None, None, false)
            .await
            .map(|rows| rows.into_iter().map(|(key, _)| key).collect())
    }

    async fn list_entries(&self, list: &ResourceList) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        self.query_list(list, None, None, true).await.map(|rows| {
            rows.into_iter()
                .filter_map(|(key, value)| value.map(|value| (key, value)))
                .collect()
        })
    }

    async fn list_keys_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ResourceKey>> {
        let _ = list
            .kind
            .as_ref()
            .ok_or_else(|| anyhow!("dynamodb list_keys_page requires a resource kind"))?;
        self.query_list(list, before_name, Some(limit), false)
            .await
            .map(|rows| rows.into_iter().map(|(key, _)| key).collect())
    }

    async fn list_entries_page(
        &self,
        list: &ResourceList,
        before_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(ResourceKey, Vec<u8>)>> {
        let _ = list
            .kind
            .as_ref()
            .ok_or_else(|| anyhow!("dynamodb list_entries_page requires a resource kind"))?;
        self.query_list(list, before_name, Some(limit), true)
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
    use super::*;

    #[test]
    fn session_keys_project_resource_parent_into_partition_key() {
        let key = crate::control::keys::session("acme:team", "agent-1", "session-1");

        assert_eq!(
            pk_for_key(&key),
            "Namespace/acme%3Ateam/Resource/Agent/agent-1"
        );
        assert_eq!(sk_for_key(&key), "Session/session-1");
    }

    #[test]
    fn journal_keys_group_under_submission_parent() {
        let key = crate::control::keys::session_journal_entry(
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
        let key = crate::control::keys::session("acme:team", "agent-1", "session-1");

        assert_eq!(
            resource_key_from_pk_sk(&pk_for_key(&key), &sk_for_key(&key)).unwrap(),
            key
        );
    }
}
