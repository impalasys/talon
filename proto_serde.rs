// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use struct_proto::google::protobuf::{value, ListValue, Value};

type CapabilitiesPolicyManifest = HashMap<String, Vec<String>>;

fn capabilities_policy_into_proto(
    policy: CapabilitiesPolicyManifest,
) -> HashMap<String, ListValue> {
    policy
        .into_iter()
        .map(|(name, actions)| {
            (
                name,
                ListValue {
                    values: actions
                        .into_iter()
                        .map(|action| Value {
                            kind: Some(value::Kind::StringValue(action)),
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

fn capabilities_policy_from_proto(
    policy: &HashMap<String, ListValue>,
) -> CapabilitiesPolicyManifest {
    policy
        .iter()
        .map(|(name, actions)| {
            (
                name.clone(),
                actions
                    .values
                    .iter()
                    .filter_map(|value| match value.kind.as_ref() {
                        Some(value::Kind::StringValue(action)) => Some(action.clone()),
                        _ => None,
                    })
                    .collect(),
            )
        })
        .collect()
}

pub mod capabilities_policy_serde {
    use super::*;

    pub fn serialize<S>(
        policy: &HashMap<String, ListValue>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        capabilities_policy_from_proto(policy).serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, ListValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let policy = CapabilitiesPolicyManifest::deserialize(deserializer)?;
        Ok(capabilities_policy_into_proto(policy))
    }
}
