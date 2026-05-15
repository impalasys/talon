// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

pub mod mcp;

pub use mcp::{McpClient, McpTool};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Lead {
    pub name: String,
    pub email: String,
    pub company: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Deal {
    pub id: String,
    pub amount: f64,
    pub status: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Subscription {
    pub id: String,
    pub customer_id: String,
    pub plan_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Invoice {
    pub id: String,
    pub amount: f64,
    pub due_date: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CalendarEvent {
    pub title: String,
    pub start_time: String,
    pub duration_mins: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub content: String,
}

#[cfg(test)]
mod mcp_tests;
