// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use serde::{de, Deserialize, Deserializer, Serializer};

use crate::gateway::rpc::resources_proto;

fn normalize_enum_name(value: &str) -> String {
    value.trim().replace('-', "_").to_ascii_uppercase()
}

fn deserialize_enum_i32<'de, D, F>(deserializer: D, name: &str, parse: F) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
    F: FnOnce(EnumValue) -> Result<i32, String>,
{
    let value = EnumValue::deserialize(deserializer)?;
    parse(value).map_err(|message| de::Error::custom(format!("invalid {name}: {message}")))
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EnumValue {
    String(String),
    Number(i32),
}

pub mod file_purpose {
    use super::*;

    pub fn serialize<S>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match resources_proto::FilePurpose::try_from(*value).ok() {
            Some(resources_proto::FilePurpose::Memory) => "MEMORY",
            Some(resources_proto::FilePurpose::Artifact) => "ARTIFACT",
            _ => "UNSPECIFIED",
        })
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum_i32(deserializer, "FilePurpose", |value| match value {
            EnumValue::Number(number) => resources_proto::FilePurpose::try_from(number)
                .map(|_| number)
                .map_err(|_| format!("unsupported numeric value {number}")),
            EnumValue::String(value) => {
                match normalize_enum_name(&value).trim_start_matches("FILE_PURPOSE_") {
                    "" | "UNSPECIFIED" => Ok(resources_proto::FilePurpose::Unspecified as i32),
                    "MEMORY" => Ok(resources_proto::FilePurpose::Memory as i32),
                    "ARTIFACT" => Ok(resources_proto::FilePurpose::Artifact as i32),
                    other => Err(format!("unsupported value '{other}'")),
                }
            }
        })
    }
}

pub mod file_index_policy {
    use super::*;

    pub fn serialize<S>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(
            match resources_proto::FileIndexPolicy::try_from(*value).ok() {
                Some(resources_proto::FileIndexPolicy::None) => "NONE",
                Some(resources_proto::FileIndexPolicy::Search) => "SEARCH",
                Some(resources_proto::FileIndexPolicy::Retrieval) => "RETRIEVAL",
                _ => "UNSPECIFIED",
            },
        )
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum_i32(deserializer, "FileIndexPolicy", |value| match value {
            EnumValue::Number(number) => resources_proto::FileIndexPolicy::try_from(number)
                .map(|_| number)
                .map_err(|_| format!("unsupported numeric value {number}")),
            EnumValue::String(value) => {
                match normalize_enum_name(&value).trim_start_matches("FILE_INDEX_POLICY_") {
                    "" | "UNSPECIFIED" => Ok(resources_proto::FileIndexPolicy::Unspecified as i32),
                    "NONE" => Ok(resources_proto::FileIndexPolicy::None as i32),
                    "SEARCH" => Ok(resources_proto::FileIndexPolicy::Search as i32),
                    "RETRIEVAL" => Ok(resources_proto::FileIndexPolicy::Retrieval as i32),
                    other => Err(format!("unsupported value '{other}'")),
                }
            }
        })
    }
}

pub mod file_retention {
    use super::*;

    pub fn serialize<S>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(
            match resources_proto::FileRetention::try_from(*value).ok() {
                Some(resources_proto::FileRetention::Retained) => "RETAINED",
                _ => "UNSPECIFIED",
            },
        )
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum_i32(deserializer, "FileRetention", |value| match value {
            EnumValue::Number(number) => resources_proto::FileRetention::try_from(number)
                .map(|_| number)
                .map_err(|_| format!("unsupported numeric value {number}")),
            EnumValue::String(value) => {
                match normalize_enum_name(&value).trim_start_matches("FILE_RETENTION_") {
                    "" | "UNSPECIFIED" => Ok(resources_proto::FileRetention::Unspecified as i32),
                    "RETAINED" => Ok(resources_proto::FileRetention::Retained as i32),
                    other => Err(format!("unsupported value '{other}'")),
                }
            }
        })
    }
}

pub mod task_phase {
    use super::*;

    pub fn serialize<S>(value: &i32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match resources_proto::TaskPhase::try_from(*value).ok() {
            Some(resources_proto::TaskPhase::Queued) => "QUEUED",
            Some(resources_proto::TaskPhase::Running) => "RUNNING",
            Some(resources_proto::TaskPhase::Blocked) => "BLOCKED",
            Some(resources_proto::TaskPhase::NeedsReview) => "NEEDS_REVIEW",
            Some(resources_proto::TaskPhase::Succeeded) => "SUCCEEDED",
            Some(resources_proto::TaskPhase::Failed) => "FAILED",
            Some(resources_proto::TaskPhase::Canceled) => "CANCELED",
            Some(resources_proto::TaskPhase::Expired) => "EXPIRED",
            _ => "UNSPECIFIED",
        })
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i32, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum_i32(deserializer, "TaskPhase", |value| match value {
            EnumValue::Number(number) => resources_proto::TaskPhase::try_from(number)
                .map(|_| number)
                .map_err(|_| format!("unsupported numeric value {number}")),
            EnumValue::String(value) => {
                match normalize_enum_name(&value).trim_start_matches("TASK_PHASE_") {
                    "" | "UNSPECIFIED" => Ok(resources_proto::TaskPhase::Unspecified as i32),
                    "QUEUED" => Ok(resources_proto::TaskPhase::Queued as i32),
                    "RUNNING" => Ok(resources_proto::TaskPhase::Running as i32),
                    "BLOCKED" => Ok(resources_proto::TaskPhase::Blocked as i32),
                    "NEEDS_REVIEW" => Ok(resources_proto::TaskPhase::NeedsReview as i32),
                    "SUCCEEDED" | "SUCCESS" | "COMPLETED" => {
                        Ok(resources_proto::TaskPhase::Succeeded as i32)
                    }
                    "FAILED" => Ok(resources_proto::TaskPhase::Failed as i32),
                    "CANCELED" | "CANCELLED" => Ok(resources_proto::TaskPhase::Canceled as i32),
                    "EXPIRED" => Ok(resources_proto::TaskPhase::Expired as i32),
                    other => Err(format!("unsupported value '{other}'")),
                }
            }
        })
    }
}
