// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::path::PathBuf;
use talon_server::{authorization_header, Options, Server};

#[test]
fn options_default_has_startup_timeout() {
    assert!(Options::default().startup_timeout.as_secs() > 0);
}

#[test]
fn options_default_has_generated_config() {
    let options = Options::default();
    assert!(options.config_path.is_none());
    assert!(options.config.is_none());
    assert!(options.data_dir.is_none());
}

#[test]
fn start_rejects_ambiguous_config_options() {
    let result = Server::start(Options {
        config_path: Some(PathBuf::from("talon.yaml")),
        config: Some(serde_json::json!({"workspace_dir": "."})),
        ..Options::default()
    });
    match result {
        Ok(_) => panic!("expected config validation error"),
        Err(err) => assert!(err.to_string().contains("config_path cannot be combined")),
    }
}

#[test]
fn authorization_header_formats_bearer_token() {
    let token = "test-token";
    assert_eq!(
        authorization_header(token).unwrap(),
        format!("Bearer {token}")
    );
}

#[test]
fn authorization_header_requires_token() {
    let err = authorization_header(" ").unwrap_err();
    assert!(err.to_string().contains("token is required"));
}
