// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use clap::{Args, Subcommand};

use super::{Cli, RunOutcome};
use crate::cli::{
    read_json_arg, workflow_run_cancel, workflow_run_create, workflow_run_events, workflow_run_get,
    workflow_run_list, workflow_run_resume,
};

#[derive(Args)]
pub(crate) struct WorkflowCommand {
    #[command(subcommand)]
    command: WorkflowCommands,
}

#[derive(Subcommand)]
enum WorkflowCommands {
    /// Create a workflow run.
    RunCreate {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, conflicts_with = "input_file")]
        input: Option<String>,
        #[arg(long, conflicts_with = "input")]
        input_file: Option<String>,
    },
    /// Get one workflow run and its step runs.
    RunGet {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// List workflow runs.
    RunList {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        #[arg(long, default_value_t = 0)]
        page_size: i32,
        #[arg(long, default_value = "")]
        before_run_id: String,
    },
    /// Resume a suspended workflow step.
    RunResume {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
        step_id: String,
        #[arg(long, conflicts_with = "resume_file")]
        resume: Option<String>,
        #[arg(long, conflicts_with = "resume")]
        resume_file: Option<String>,
    },
    /// Cancel a workflow run.
    RunCancel {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
    /// Stream workflow run events.
    RunEvents {
        #[arg(short, long)]
        namespace: String,
        workflow: String,
        run_id: String,
    },
}

pub(super) async fn run(cli: &Cli, command: &WorkflowCommand) -> Result<RunOutcome> {
    match &command.command {
        WorkflowCommands::RunCreate {
            namespace,
            workflow,
            input,
            input_file,
        } => {
            let value =
                workflow_run_create(cli, namespace, workflow, read_json_arg(input, input_file)?)
                    .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunGet {
            namespace,
            workflow,
            run_id,
        } => {
            let value = workflow_run_get(cli, namespace, workflow, run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunList {
            namespace,
            workflow,
            page_size,
            before_run_id,
        } => {
            let value =
                workflow_run_list(cli, namespace, workflow, *page_size, before_run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunResume {
            namespace,
            workflow,
            run_id,
            step_id,
            resume,
            resume_file,
        } => {
            let value = workflow_run_resume(
                cli,
                namespace,
                workflow,
                run_id,
                step_id,
                read_json_arg(resume, resume_file)?,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunCancel {
            namespace,
            workflow,
            run_id,
        } => {
            let value = workflow_run_cancel(cli, namespace, workflow, run_id).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
        WorkflowCommands::RunEvents {
            namespace,
            workflow,
            run_id,
        } => {
            workflow_run_events(cli, namespace, workflow, run_id).await?;
        }
    }
    Ok(RunOutcome { exit_code: None })
}
