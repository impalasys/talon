// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::time::Duration;

use super::{Cli, RunOutcome};
use crate::cli::connect_gateway;
use talon_client::v1::{
    CompleteFileUploadRequest, CreateFileRequest, DeleteFileRequest, FileRef, ListFilesRequest,
    PrepareFileUploadRequest, ReadFileRequest, UpdateFileRequest,
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
    /// Read a namespace File by name, path, or URI.
    Get {
        #[arg(short, long)]
        namespace: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        uri: Option<String>,
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
        uri: Option<String>,
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
    /// Delete a namespace File by name, path, or URI.
    Delete {
        #[arg(short, long)]
        namespace: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        uri: Option<String>,
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
            let media_type = media_type_for_create(file, media_type.as_deref());
            let mut client = connect_gateway(cli).await?;
            let purpose = parse_purpose(purpose)?;
            let index_policy = parse_index_policy(index_policy)?;
            let retention = talon_client::resources::FileRetention::Retained as i32;
            let response = if bytes.len() as u64 <= MAX_UNARY_FILE_UPLOAD_BYTES {
                client
                    .create_file(CreateFileRequest {
                        namespace: namespace.clone(),
                        path: path.clone(),
                        media_type,
                        purpose,
                        index_policy,
                        retention,
                        content: bytes,
                    })
                    .await?
                    .into_inner()
            } else {
                signed_upload_create(
                    &mut client,
                    namespace,
                    path,
                    media_type,
                    purpose,
                    index_policy,
                    retention,
                    &bytes,
                )
                .await?
            };
            let file = response.file.context("FileService returned no file")?;
            println!(
                "✓ File '{}/{}' written. uri={}",
                namespace,
                file.metadata
                    .as_ref()
                    .map(|meta| meta.name.as_str())
                    .unwrap_or_default(),
                response.file_uri
            );
        }
        FileCommands::Get {
            namespace,
            name,
            path,
            uri,
            output,
        } => {
            let mut client = connect_gateway(cli).await?;
            let response = client
                .read_file(ReadFileRequest {
                    file: Some(file_ref(namespace, name, path, uri)?),
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
            uri,
            file,
            media_type,
        } => {
            let bytes = fs::read(file).with_context(|| format!("Failed to read '{}'", file))?;
            let media_type = media_type_for_update(file, media_type.as_deref());
            let mut client = connect_gateway(cli).await?;
            let file_ref = file_ref(namespace, name, path, uri)?;
            let response = if bytes.len() as u64 <= MAX_UNARY_FILE_UPLOAD_BYTES {
                client
                    .update_file(UpdateFileRequest {
                        file: Some(file_ref),
                        media_type,
                        content: bytes,
                    })
                    .await?
                    .into_inner()
            } else {
                signed_upload_update(&mut client, file_ref, media_type, &bytes).await?
            };
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
            uri,
        } => {
            let mut client = connect_gateway(cli).await?;
            let response = client
                .delete_file(DeleteFileRequest {
                    file: Some(file_ref(namespace, name, path, uri)?),
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
    uri: &Option<String>,
) -> Result<FileRef> {
    let namespace = namespace.clone().unwrap_or_default();
    let name = name.clone().unwrap_or_default();
    let path = path.clone().unwrap_or_default();
    let uri = uri.clone().unwrap_or_default();
    if name.trim().is_empty() && path.trim().is_empty() && uri.trim().is_empty() {
        anyhow::bail!("one of --name, --path, or --uri is required");
    }
    if uri.trim().is_empty() && namespace.trim().is_empty() {
        anyhow::bail!("--namespace is required unless --uri is provided");
    }
    Ok(FileRef {
        namespace,
        name,
        path,
        uri,
    })
}

async fn read_file_response_content(response: talon_client::v1::ReadFileResponse) -> Result<Vec<u8>> {
    if !response.signed_url.trim().is_empty() {
        return download_signed_url(&response.signed_url).await;
    }
    Ok(response.content)
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

async fn signed_upload_create(
    client: &mut talon_client::TalonClient,
    namespace: &str,
    path: &str,
    media_type: String,
    purpose: i32,
    index_policy: i32,
    retention: i32,
    bytes: &[u8],
) -> Result<talon_client::v1::FileResponse> {
    let response = client
        .prepare_file_upload(PrepareFileUploadRequest {
            namespace: namespace.to_string(),
            path: path.to_string(),
            media_type,
            purpose,
            index_policy,
            retention,
            file: None,
            expected_size_bytes: bytes.len() as u64,
            expected_sha256: sha256_hex(bytes),
        })
        .await?
        .into_inner();
    upload_to_signed_url(&response.signed_upload_url, &response.required_headers, bytes).await?;
    Ok(client
        .complete_file_upload(CompleteFileUploadRequest {
            upload_token: response.upload_token,
        })
        .await?
        .into_inner())
}

async fn signed_upload_update(
    client: &mut talon_client::TalonClient,
    file: FileRef,
    media_type: String,
    bytes: &[u8],
) -> Result<talon_client::v1::FileResponse> {
    let response = client
        .prepare_file_upload(PrepareFileUploadRequest {
            namespace: String::new(),
            path: String::new(),
            media_type,
            purpose: 0,
            index_policy: 0,
            retention: 0,
            file: Some(file),
            expected_size_bytes: bytes.len() as u64,
            expected_sha256: sha256_hex(bytes),
        })
        .await?
        .into_inner();
    upload_to_signed_url(&response.signed_upload_url, &response.required_headers, bytes).await?;
    Ok(client
        .complete_file_upload(CompleteFileUploadRequest {
            upload_token: response.upload_token,
        })
        .await?
        .into_inner())
}

async fn upload_to_signed_url(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
    bytes: &[u8],
) -> Result<()> {
    if url.trim().is_empty() {
        anyhow::bail!(
            "gateway did not return a signed upload URL; configured object store may not support signed uploads"
        );
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to build signed URL upload client")?;
    let mut request = client.put(url).body(bytes.to_vec());
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
        .send()
        .await
        .context("Failed to upload file to signed URL")?
        .error_for_status()
        .context("Signed upload URL returned an error")?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
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
