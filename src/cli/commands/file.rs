// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use futures::StreamExt;
use std::fs;
use std::io::Write;
use std::time::Duration;

use super::{Cli, RunOutcome};
use crate::cli::connect_gateway;
use talon_client::v1::{
    CreateFileRequest, DeleteFileRequest, FileRef, ListFilesRequest, ReadFileRequest,
    UpdateFileRequest,
};

const MAX_SIGNED_URL_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_UNARY_FILE_UPLOAD_BYTES: u64 = 3 * 1024 * 1024;

#[derive(Args)]
pub(crate) struct FileCommand {
    #[command(subcommand)]
    command: FileCommands,
}

#[derive(Subcommand)]
enum FileCommands {
    /// Create a namespace File from a local file.
    Put {
        #[arg(short, long)]
        namespace: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        file: String,
        #[arg(long)]
        media_type: Option<String>,
        #[arg(long, default_value = "artifact")]
        purpose: String,
        #[arg(long, default_value = "none")]
        index_policy: String,
    },
    /// Read a namespace File by name, path, or handle.
    Get {
        #[arg(short, long)]
        namespace: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        handle: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Update a namespace File's bytes.
    Update {
        #[arg(short, long)]
        namespace: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        handle: Option<String>,
        #[arg(long)]
        file: String,
        #[arg(long)]
        media_type: Option<String>,
    },
    /// List namespace Files.
    List {
        #[arg(short, long)]
        namespace: String,
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        purpose: Option<String>,
        #[arg(long)]
        index_policy: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Delete a namespace File by name, path, or handle.
    Delete {
        #[arg(short, long)]
        namespace: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        handle: Option<String>,
    },
}

pub(super) async fn run(cli: &Cli, command: &FileCommand) -> Result<RunOutcome> {
    match &command.command {
        FileCommands::Put {
            namespace,
            path,
            file,
            media_type,
            purpose,
            index_policy,
        } => {
            let bytes = fs::read(file).with_context(|| format!("Failed to read '{}'", file))?;
            ensure_unary_upload_size(bytes.len() as u64)?;
            let media_type = media_type_for_create(file, media_type.as_deref());
            let mut client = connect_gateway(cli).await?;
            let response = client
                .create_file(CreateFileRequest {
                    namespace: namespace.clone(),
                    path: path.clone(),
                    media_type,
                    purpose: parse_purpose(purpose)?,
                    index_policy: parse_index_policy(index_policy)?,
                    retention: talon_client::resources::FileRetention::Retained as i32,
                    content: bytes,
                })
                .await?
                .into_inner();
            let file = response.file.context("FileService returned no file")?;
            println!(
                "✓ File '{}/{}' written. handle={}",
                namespace,
                file.metadata
                    .as_ref()
                    .map(|meta| meta.name.as_str())
                    .unwrap_or_default(),
                response.file_handle
            );
        }
        FileCommands::Get {
            namespace,
            name,
            path,
            handle,
            output,
        } => {
            let mut client = connect_gateway(cli).await?;
            let response = client
                .read_file(ReadFileRequest {
                    file: Some(file_ref(namespace, name, path, handle)?),
                })
                .await?
                .into_inner();
            let content = read_file_response_content(response).await?;
            if let Some(output) = output {
                fs::write(output, &content)
                    .with_context(|| format!("Failed to write '{}'", output))?;
            } else {
                std::io::stdout()
                    .write_all(&content)
                    .context("Failed to write file content to stdout")?;
            }
        }
        FileCommands::Update {
            namespace,
            name,
            path,
            handle,
            file,
            media_type,
        } => {
            let bytes = fs::read(file).with_context(|| format!("Failed to read '{}'", file))?;
            ensure_unary_upload_size(bytes.len() as u64)?;
            let media_type = media_type_for_update(file, media_type.as_deref());
            let mut client = connect_gateway(cli).await?;
            let response = client
                .update_file(UpdateFileRequest {
                    file: Some(file_ref(namespace, name, path, handle)?),
                    media_type,
                    content: bytes,
                })
                .await?
                .into_inner();
            let file = response.file.context("FileService returned no file")?;
            println!(
                "✓ File '{}' updated.",
                file.metadata
                    .as_ref()
                    .map(|meta| meta.name.as_str())
                    .unwrap_or_default()
            );
        }
        FileCommands::List {
            namespace,
            prefix,
            purpose,
            index_policy,
            limit,
        } => {
            let mut client = connect_gateway(cli).await?;
            let purpose = purpose
                .as_deref()
                .map(parse_purpose)
                .transpose()?
                .unwrap_or_default();
            let index_policy = index_policy
                .as_deref()
                .map(parse_index_policy)
                .transpose()?
                .unwrap_or_default();
            let mut remaining = (*limit).max(1) as usize;
            let mut page_token = String::new();
            while remaining > 0 {
                let page_limit = remaining.min(200) as u32;
                let response = client
                    .list_files(ListFilesRequest {
                        namespace: namespace.clone(),
                        prefix: prefix.clone(),
                        purpose,
                        index_policy,
                        limit: page_limit,
                        page_token,
                    })
                    .await?
                    .into_inner();
                page_token = response.next_page_token;
                for file in response.files {
                    if remaining == 0 {
                        break;
                    }
                    let spec = file.spec.as_ref();
                    let status = file.status.as_ref();
                    println!(
                        "{}\t{}\t{}\t{}",
                        file.metadata
                            .as_ref()
                            .map(|meta| meta.name.as_str())
                            .unwrap_or_default(),
                        spec.map(|spec| spec.path.as_str()).unwrap_or_default(),
                        spec.map(|spec| spec.media_type.as_str())
                            .unwrap_or_default(),
                        status
                            .and_then(|status| status.object_ref.as_ref())
                            .map(|object| object.size_bytes)
                            .unwrap_or_default()
                    );
                    remaining -= 1;
                }
                if page_token.is_empty() {
                    break;
                }
            }
        }
        FileCommands::Delete {
            namespace,
            name,
            path,
            handle,
        } => {
            let mut client = connect_gateway(cli).await?;
            let response = client
                .delete_file(DeleteFileRequest {
                    file: Some(file_ref(namespace, name, path, handle)?),
                })
                .await?
                .into_inner();
            println!("✓ Deleted: {}", response.success);
        }
    }
    Ok(RunOutcome { exit_code: None })
}

fn file_ref(
    namespace: &Option<String>,
    name: &Option<String>,
    path: &Option<String>,
    handle: &Option<String>,
) -> Result<FileRef> {
    let namespace = namespace.clone().unwrap_or_default();
    let name = name.clone().unwrap_or_default();
    let path = path.clone().unwrap_or_default();
    let handle = handle.clone().unwrap_or_default();
    if name.trim().is_empty() && path.trim().is_empty() && handle.trim().is_empty() {
        anyhow::bail!("one of --name, --path, or --handle is required");
    }
    if handle.trim().is_empty() && namespace.trim().is_empty() {
        anyhow::bail!("--namespace is required unless --handle is provided");
    }
    Ok(FileRef {
        namespace,
        name,
        path,
        handle,
    })
}

async fn read_file_response_content(response: talon_client::v1::ReadFileResponse) -> Result<Vec<u8>> {
    if !response.signed_url.trim().is_empty() {
        return download_signed_url(&response.signed_url).await;
    }
    Ok(response.content)
}

fn ensure_unary_upload_size(size_bytes: u64) -> Result<()> {
    if size_bytes > MAX_UNARY_FILE_UPLOAD_BYTES {
        anyhow::bail!(
            "file is {} bytes; unary file upload cap is {} bytes",
            size_bytes,
            MAX_UNARY_FILE_UPLOAD_BYTES
        );
    }
    Ok(())
}

fn media_type_for_create(file: &str, explicit: Option<&str>) -> String {
    explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            mime_guess::from_path(file)
                .first_raw()
                .unwrap_or("application/octet-stream")
                .to_string()
        })
}

fn media_type_for_update(file: &str, explicit: Option<&str>) -> String {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    mime_guess::from_path(file)
        .first_raw()
        .filter(|value| *value != "application/octet-stream")
        .unwrap_or_default()
        .to_string()
}

async fn download_signed_url(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("Failed to build signed URL download client")?;
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download signed file URL")?
        .error_for_status()
        .context("Signed file URL returned an error")?;
    if let Some(length) = response.content_length() {
        if length > MAX_SIGNED_URL_DOWNLOAD_BYTES {
            anyhow::bail!(
                "signed file URL is {} bytes; CLI download cap is {} bytes",
                length,
                MAX_SIGNED_URL_DOWNLOAD_BYTES
            );
        }
    }
    let capacity = response
        .content_length()
        .map(|length| length.min(MAX_SIGNED_URL_DOWNLOAD_BYTES) as usize)
        .unwrap_or_default();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Failed to read signed file URL body")?;
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .context("signed file URL body is too large")?;
        if next_len as u64 > MAX_SIGNED_URL_DOWNLOAD_BYTES {
            anyhow::bail!(
                "signed file URL exceeded CLI download cap of {} bytes",
                MAX_SIGNED_URL_DOWNLOAD_BYTES
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_purpose(value: &str) -> Result<i32> {
    match value.to_ascii_lowercase().as_str() {
        "memory" => Ok(talon_client::resources::FilePurpose::Memory as i32),
        "artifact" => Ok(talon_client::resources::FilePurpose::Artifact as i32),
        other => anyhow::bail!("unknown purpose '{}'", other),
    }
}

fn parse_index_policy(value: &str) -> Result<i32> {
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(talon_client::resources::FileIndexPolicy::None as i32),
        "search" => Ok(talon_client::resources::FileIndexPolicy::Search as i32),
        "retrieval" => Ok(talon_client::resources::FileIndexPolicy::Retrieval as i32),
        other => anyhow::bail!("unknown index policy '{}'", other),
    }
}
