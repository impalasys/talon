// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use clap::{Args, Subcommand};

use super::{Cli, RunOutcome};
use crate::cli::connect_gateway;
use talon_client::v1::{
    SearchKnowledgeRequest, SearchMode, SearchRequest, SearchResult, SearchSort, SearchSourceFilter,
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
                    query: query.clone(),
                    source: Some(SearchSourceFilter {
                        namespaces: vec![namespace.clone()],
                        kinds: resource_kinds.clone(),
                        ..Default::default()
                    }),
                    attributes: [
                        ("agent".to_string(), agent.clone().unwrap_or_default()),
                        (
                            "session_id".to_string(),
                            session.clone().unwrap_or_default(),
                        ),
                        ("channel".to_string(), channel.clone().unwrap_or_default()),
                    ]
                    .into_iter()
                    .filter(|(_, value)| !value.is_empty())
                    .collect(),
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
                    query: query.clone(),
                    source: Some(SearchSourceFilter {
                        namespaces: vec![namespace.clone()],
                        kinds: vec!["SessionMessage".to_string()],
                        ..Default::default()
                    }),
                    attributes: [
                        ("agent".to_string(), agent.clone()),
                        (
                            "session_id".to_string(),
                            session.clone().unwrap_or_default(),
                        ),
                    ]
                    .into_iter()
                    .filter(|(_, value)| !value.is_empty())
                    .collect(),
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

fn print_results(results: Vec<SearchResult>) {
    for result in results {
        let Some(document) = result.document else {
            continue;
        };
        let source = document.source.as_ref();
        println!(
            "{}\t{}\t{}\t{}\t{:.3}\t{}",
            source.map(|source| source.namespace.as_str()).unwrap_or(""),
            source.map(|source| source.kind.as_str()).unwrap_or(""),
            document.document_kind,
            document.id,
            result.score,
            result.snippet.replace(['\n', '\t'], " ")
        );
    }
}
