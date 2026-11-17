// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod enum_serde;

use prost_types::{value, ListValue, Value};
type CapabilitiesPolicyManifest = std::collections::HashMap<String, Vec<String>>;

fn capabilities_policy_into_proto(
    policy: CapabilitiesPolicyManifest,
) -> std::collections::HashMap<String, ListValue> {
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
    policy: &std::collections::HashMap<String, ListValue>,
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
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        policy: &std::collections::HashMap<String, ListValue>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        capabilities_policy_from_proto(policy).serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<std::collections::HashMap<String, ListValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let policy = CapabilitiesPolicyManifest::deserialize(deserializer)?;
        Ok(capabilities_policy_into_proto(policy))
    }
}
