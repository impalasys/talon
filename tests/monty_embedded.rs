// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::{env, path::PathBuf, time::Duration};

use monty_pool::{on_print_sync, Pool, PoolConfig, ReplConfig, TurnEvent};
use monty_types::MontyObject;

async fn executes_code_in(binary: PathBuf) {
    let mut config = PoolConfig::subprocess(binary);
    config.min_processes = 0;
    config.max_processes = 1;
    config.checkout_timeout = Some(Duration::from_secs(5));
    config.request_timeout = Some(Duration::from_secs(5));

    let pool = Pool::new(config)
        .await
        .expect("embedded worker pool starts");
    let mut session = pool
        .checkout(&ReplConfig::default())
        .await
        .expect("embedded worker checkout succeeds");
    let mut on_print = on_print_sync(|_, _| {});
    let event = session
        .feed("1 + 1", vec![], vec![], true, &mut on_print)
        .await
        .expect("embedded worker executes code");

    assert!(matches!(event, TurnEvent::Complete(MontyObject::Int(2))));
}

#[tokio::test]
async fn talon_host_binaries_embed_monty_worker() {
    executes_code_in(PathBuf::from(env!("CARGO_BIN_EXE_talon-node"))).await;
    executes_code_in(PathBuf::from(env!("CARGO_BIN_EXE_talon-worker"))).await;
}
