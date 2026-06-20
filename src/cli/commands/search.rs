// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use clap::{Args, Subcommand};

use super::{Cli, RunOutcome};
use crate::cli::connect_gateway;
use crate::gateway::rpc::proto::{
    SearchKnowledgeRequest, SearchMode, SearchRequest, SearchSort,
};

#[derive(Args)]
pub(crate) struct SearchCommand {
    #[command(subcommand)]
    command: SearchCommands,
}

#[derive(Subcommand)]
enum SearchCommands {
    /// Search all indexed workspace resources in a namespace.
    Workspace {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long = "kind")]
        resource_kinds: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: i32,
        query: String,
    },
    /// Search prior session messages.
    Sessions {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long)]
        agent: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: i32,
        query: String,
    },
    /// Search indexed knowledge artifacts.
    Knowledge {
        #[arg(short, long)]
        namespace: String,
        #[arg(short, long, default_value = "default")]
        agent: String,
        #[arg(long, default_value_t = 10)]
        limit: i32,
        query: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &SearchCommand) -> Result<RunOutcome> {
    if cli.rest {
        anyhow::bail!("talon-cli search currently requires gRPC; REST search is not implemented");
    }
    let mut client = connect_gateway(cli).await?;
    match &command.command {
        SearchCommands::Workspace {
            namespace,
            agent,
            session,
            channel,
            resource_kinds,
            limit,
            query,
        } => {
            let response = client
                .search(SearchRequest {
                    ns: namespace.clone(),
                    query: query.clone(),
                    resource_kinds: resource_kinds.clone(),
                    agent: agent.clone().unwrap_or_default(),
                    session_id: session.clone().unwrap_or_default(),
                    channel: channel.clone().unwrap_or_default(),
                    role: String::new(),
                    part_type: String::new(),
                    labels: Default::default(),
                    start_time: None,
                    end_time: None,
                    limit: *limit,
                    page_token: String::new(),
                    mode: SearchMode::Keyword as i32,
                    sort: SearchSort::Relevance as i32,
                })
                .await?
                .into_inner();
            print_results(response.results);
        }
        SearchCommands::Sessions {
            namespace,
            agent,
            session,
            limit,
            query,
        } => {
            let response = client
                .search(SearchRequest {
                    ns: namespace.clone(),
                    query: query.clone(),
                    resource_kinds: vec!["SessionMessage".to_string()],
                    agent: agent.clone(),
                    session_id: session.clone().unwrap_or_default(),
                    channel: String::new(),
                    role: String::new(),
                    part_type: String::new(),
                    labels: Default::default(),
                    start_time: None,
                    end_time: None,
                    limit: *limit,
                    page_token: String::new(),
                    mode: SearchMode::Keyword as i32,
                    sort: SearchSort::Relevance as i32,
                })
                .await?
                .into_inner();
            print_results(response.results);
        }
        SearchCommands::Knowledge {
            namespace,
            agent,
            limit,
            query,
        } => {
            let response = client
                .search_knowledge(SearchKnowledgeRequest {
                    ns: namespace.clone(),
                    agent: agent.clone(),
                    query: query.clone(),
                    limit: *limit,
                    mode: SearchMode::Keyword as i32,
                    sort: SearchSort::Relevance as i32,
                })
                .await?
                .into_inner();
            print_results(response.search_results);
        }
    }
    Ok(RunOutcome { exit_code: None })
}

fn print_results(results: Vec<crate::gateway::rpc::proto::SearchResult>) {
    for result in results {
        let Some(document) = result.document else {
            continue;
        };
        println!(
            "{}\t{}\t{}\t{}\t{:.3}\t{}",
            document.namespace,
            document.resource_kind,
            document.document_kind,
            document.id,
            result.score,
            document.snippet.replace('\n', " ")
        );
    }
}
