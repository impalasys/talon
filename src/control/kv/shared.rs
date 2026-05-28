// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use std::path::Path;

const RESERVED_IDENTIFIERS: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "between",
    "binary",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "grant",
    "group",
    "having",
    "in",
    "initially",
    "intersect",
    "into",
    "is",
    "leading",
    "limit",
    "localtime",
    "localtimestamp",
    "not",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "placing",
    "primary",
    "references",
    "returning",
    "select",
    "session_user",
    "some",
    "symmetric",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "when",
    "where",
    "window",
    "with",
];

pub(crate) fn validate_identifier(table: &str) -> Result<()> {
    if table.is_empty() || table.len() > 63 {
        anyhow::bail!(
            "Invalid table name '{}': must be between 1 and 63 characters",
            table
        );
    }
    let mut chars = table.chars();
    let first = chars.next().expect("table name is not empty");
    if !first.is_ascii_alphabetic() && first != '_' {
        anyhow::bail!(
            "Invalid table name '{}': must start with a letter or underscore",
            table
        );
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        anyhow::bail!(
            "Invalid table name '{}': only ASCII letters, numbers, and underscores are allowed",
            table
        );
    }
    if RESERVED_IDENTIFIERS
        .binary_search(&table.to_ascii_lowercase().as_str())
        .is_ok()
    {
        anyhow::bail!(
            "Invalid table name '{}': SQL reserved keywords are not allowed",
            table
        );
    }
    Ok(())
}

pub(crate) fn quoted_identifier(table: &str) -> String {
    format!("\"{}\"", table)
}

pub fn sqlite_url_for_path(path: &Path) -> String {
    format!("sqlite://{}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{sqlite_url_for_path, validate_identifier};
    use std::path::Path;

    #[test]
    fn validate_identifier_rejects_invalid_table_names() {
        validate_identifier("talon_kv").expect("underscores should be allowed");
        validate_identifier("talon123").expect("alphanumeric names should be allowed");
        validate_identifier("_talon").expect("leading underscore should be allowed");

        let err = validate_identifier("talon-kv").unwrap_err();
        assert!(err.to_string().contains("Invalid table name"));

        let empty = validate_identifier("").unwrap_err();
        assert!(empty.to_string().contains("Invalid table name"));

        let starts_with_digit = validate_identifier("1talon").unwrap_err();
        assert!(starts_with_digit.to_string().contains("must start"));

        let too_long = validate_identifier(&"a".repeat(64)).unwrap_err();
        assert!(too_long.to_string().contains("between 1 and 63"));

        let reserved = validate_identifier("select").unwrap_err();
        assert!(reserved.to_string().contains("reserved keywords"));

        validate_identifier("select_log").expect("non-keyword identifiers should be allowed");
    }

    #[test]
    fn sqlite_url_formats_paths() {
        assert_eq!(
            sqlite_url_for_path(Path::new("/tmp/talon.db")),
            "sqlite:///tmp/talon.db"
        );
    }
}
