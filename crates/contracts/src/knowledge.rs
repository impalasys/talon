// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KnowledgeEntry {
    pub namespace: String,
    pub name: String,
    pub path: String,
    pub content: String,
    pub updated_at: i64,
}

pub struct KnowledgeResult {
    pub namespace: String,
    pub path: String,
    pub excerpt: String,
    pub updated_at: i64,
}

impl KnowledgeEntry {
    pub fn path(&self) -> String {
        if self.path.is_empty() {
            self.name.clone()
        } else {
            self.path.clone()
        }
    }
}
