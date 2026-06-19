// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::Path;

use super::{Cli, RunOutcome};
use crate::cli::{
    knowledge_delete, knowledge_get, knowledge_set, read_knowledge_content, sync_knowledge_dir,
};

#[derive(Args)]
pub(crate) struct KnowledgeCommand {
    #[command(subcommand)]
    command: KnowledgeCommands,
}

#[derive(Subcommand)]
enum KnowledgeCommands {
    /// Read a knowledge artifact by path.
    Get {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Write a knowledge artifact from inline content or a file.
    Set {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
    },
    /// Delete a knowledge artifact by path.
    Delete {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
    },
    /// Sync all markdown files in a directory into namespace knowledge.
    Sync {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        dir: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &KnowledgeCommand) -> Result<RunOutcome> {
    match &command.command {
        KnowledgeCommands::Get { namespace, path } => {
            let knowledge = knowledge_get(cli, namespace, path).await?;
            let Some(knowledge) = knowledge else {
                eprintln!("Knowledge '{}/{}' not found.", namespace, path);
                return Ok(RunOutcome { exit_code: Some(1) });
            };
            let content = knowledge
                .spec
                .as_ref()
                .map(|spec| spec.content.clone())
                .unwrap_or_default();
            print!("{}", content);
            if !content.ends_with('\n') {
                println!();
            }
        }
        KnowledgeCommands::Set {
            namespace,
            path,
            file,
            content,
        } => {
            let value = read_knowledge_content(file, content)?;
            knowledge_set(cli, namespace, path, value).await?;
            println!("✓ Knowledge '{}/{}' written successfully.", namespace, path);
        }
        KnowledgeCommands::Delete { namespace, path } => {
            knowledge_delete(cli, namespace, path).await?;
            println!("✓ Knowledge '{}/{}' deleted successfully.", namespace, path);
        }
        KnowledgeCommands::Sync { namespace, dir } => {
            let root = Path::new(dir);
            let (synced_count, unsynced_existing) = sync_knowledge_dir(cli, namespace, dir).await?;
            println!(
                "✓ Synced {} knowledge artifact(s) into '{}'.",
                synced_count, namespace
            );
            if !unsynced_existing.is_empty() {
                eprintln!(
                    "Note: {} existing knowledge artifact(s) in '{}' were left untouched because they are not present in '{}'.",
                    unsynced_existing.len(),
                    namespace,
                    root.display()
                );
            }
        }
    }
    Ok(RunOutcome { exit_code: None })
}
