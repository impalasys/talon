// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::resource_model::{self, ChannelSubscriptionResourceExt, TypedResource};
use crate::gateway::rpc::manifests::{Knowledge, KnowledgeSpec, ObjectMeta};
use crate::gateway::rpc::proto::gateway_service_client::GatewayServiceClient;
use crate::gateway::rpc::proto::{
    CancelWorkflowRunRequest, CreateResourceRequest, CreateWorkflowRunRequest,
    DeleteResourceRequest, GetResourceRequest, GetWorkflowRunRequest, ListNamespacesRequest,
    ListResourcesRequest, ListWorkflowRunsRequest, ResumeWorkflowRunRequest,
    StreamWorkflowEventsRequest,
};
use crate::gateway::rpc::{data_proto, resources_proto};
use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use minijinja::{context, Environment, UndefinedBehavior};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

mod auth;
mod commands;

use commands::Cli;

pub(super) use auth::{
    auth_interceptor, connect_gateway, mint_agent_jwt, mint_channel_jwt, mint_root_jwt,
    mint_session_jwt, resolve_gateway_jwt_secret, resolve_gateway_password, rest_client,
};
