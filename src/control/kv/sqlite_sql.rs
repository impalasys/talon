// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::shared::quoted_identifier;
use crate::control::Order;

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

pub(crate) fn list_keys_query(table: &str, has_kind: bool, order: Order) -> String {
    let kind_clause = if has_kind { "AND kind = ?3" } else { "" };
    let direction = list_order_sql(order);
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {kind_clause}
         ORDER BY kind {direction}, name {direction}",
        quoted_identifier(table)
    )
}

pub(crate) fn list_entries_query(table: &str, has_kind: bool, order: Order) -> String {
    let kind_clause = if has_kind { "AND kind = ?3" } else { "" };
    let direction = list_order_sql(order);
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 {kind_clause}
         ORDER BY kind {direction}, name {direction}",
        quoted_identifier(table)
    )
}

pub(crate) fn list_keys_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3
           AND (?4 IS NULL OR name < ?4)
         ORDER BY name DESC
         LIMIT ?5",
        quoted_identifier(table)
    )
}

pub(crate) fn list_entries_page_query(table: &str) -> String {
    format!(
        "SELECT namespace, parent_path, kind, name, value FROM {}
         WHERE namespace = ?1 AND parent_path = ?2 AND kind = ?3
           AND (?4 IS NULL OR name < ?4)
         ORDER BY name DESC
         LIMIT ?5",
        quoted_identifier(table)
    )
}

pub(crate) fn create_migration_table_statement(table: &str) -> String {
    create_table_statement(table).replacen("CREATE TABLE IF NOT EXISTS", "CREATE TABLE", 1)
}
