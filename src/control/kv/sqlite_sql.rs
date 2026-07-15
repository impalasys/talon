// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::shared::quoted_identifier;
use crate::control::{ListOptions, Order};

pub(crate) fn create_table_statement(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "CREATE TABLE IF NOT EXISTS {} (
                namespace TEXT NOT NULL,
                parent_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                value BLOB NOT NULL,
                PRIMARY KEY (namespace, parent_path, kind, name)
            ) WITHOUT ROWID",
        table
    )
}

pub(crate) fn get_query(table: &str) -> String {
    format!(
        "SELECT value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4",
        quoted_identifier(table)
    )
}

pub(crate) fn set_query(table: &str) -> String {
    let table = quoted_identifier(table);
    format!(
        "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (namespace, parent_path, kind, name)
             DO UPDATE SET value = excluded.value",
        table
    )
}

pub(crate) fn compare_and_swap_query(table: &str, expected: bool) -> String {
    let table = quoted_identifier(table);
    if expected {
        format!(
            "UPDATE {} SET value = ?5
             WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4 AND value = ?6",
            table
        )
    } else {
        format!(
            "INSERT INTO {} (namespace, parent_path, kind, name, value)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (namespace, parent_path, kind, name) DO NOTHING",
            table
        )
    }
}

pub(crate) fn delete_query(table: &str) -> String {
    format!(
        "DELETE FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3 AND name = ?4",
        quoted_identifier(table)
    )
}

fn list_order_sql(order: Order) -> &'static str {
    if order == Order::Desc {
        "DESC"
    } else {
        "ASC"
    }
}

fn list_filter_clause(has_kind: bool, options: ListOptions<'_>) -> String {
    let mut next_bind = if has_kind { 4 } else { 3 };
    let mut clauses = Vec::new();
    if has_kind {
        clauses.push("AND kind = ?3".to_string());
    }
    if options.before_name.is_some() {
        clauses.push(format!("AND name < ?{next_bind}"));
        next_bind += 1;
    }
    if options.after_name.is_some() {
        clauses.push(format!("AND name > ?{next_bind}"));
    }
    clauses.join(" ")
}

fn list_limit_clause(has_kind: bool, options: ListOptions<'_>) -> String {
    let mut next_bind = if has_kind { 4 } else { 3 };
    if options.before_name.is_some() {
        next_bind += 1;
    }
    if options.after_name.is_some() {
        next_bind += 1;
    }
    if options.limit.is_some() {
        format!(" LIMIT ?{next_bind}")
    } else {
        String::new()
    }
}

pub(crate) fn list_keys_query(table: &str, has_kind: bool, options: ListOptions<'_>) -> String {
    let filter_clause = list_filter_clause(has_kind, options);
    let limit_clause = list_limit_clause(has_kind, options);
    let direction = list_order_sql(options.order);
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {filter_clause}
         ORDER BY kind {direction}, name {direction}{limit_clause}",
        quoted_identifier(table)
    )
}

pub(crate) fn list_entries_query(table: &str, has_kind: bool, options: ListOptions<'_>) -> String {
    let filter_clause = list_filter_clause(has_kind, options);
    let limit_clause = list_limit_clause(has_kind, options);
    let direction = list_order_sql(options.order);
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {filter_clause}
         ORDER BY kind {direction}, name {direction}{limit_clause}",
        quoted_identifier(table)
    )
}

pub(crate) fn create_migration_table_statement(table: &str) -> String {
    create_table_statement(table).replacen("CREATE TABLE IF NOT EXISTS", "CREATE TABLE", 1)
}
