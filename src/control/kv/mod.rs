// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

#[cfg(feature = "dynamodb")]
mod dynamodb;
mod legacy;
mod postgres;
#[cfg(feature = "rocksdb")]
mod rocksdb;
mod shared;
mod sqlite;
mod sqlite_sql;

#[cfg(feature = "dynamodb")]
pub use dynamodb::DynamoDbKvStore;
pub use postgres::PostgresKvStore;
#[cfg(feature = "rocksdb")]
pub use rocksdb::RocksDbKvStore;
pub use shared::sqlite_url_for_path;
pub(crate) use shared::validate_identifier;
pub use sqlite::SqliteKvStore;
