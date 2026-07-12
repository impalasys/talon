// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::{data_proto, proto, resources_proto, GrpcGatewayHandler};
use crate::control::cas::{latest_file_object_key, CasStore};
use crate::control::resource_model;
use crate::control::resources::{file_resource_name_for_path, ResourceStore};
use crate::control::{keys, ProtoKeyValueStoreExt};
use crate::gateway::auth::Claims;
use crate::require_auth;
use anyhow::{anyhow, Context, Result};
use prost::Message;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;
use tonic::{metadata::MetadataMap, Request, Response, Status};

const HANDLE_KIND_ARTIFACT: &str = "ARTIFACT";
const HANDLE_KIND_FILE: &str = "FILE";
const OP_READ: &str = "read";
const OP_METADATA: &str = "metadata";
const OP_WRITE: &str = "write";
const OP_DELETE: &str = "delete";
const OP_PROMOTE: &str = "promote";
const MAX_HANDLE_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_UNARY_FILE_CONTENT_BYTES: usize = 3 * 1024 * 1024;
const FILE_LIST_SCAN_PAGE_SIZE: usize = 200;
const MAX_FILE_LIST_SCAN_PAGES: usize = 25;
const FILE_SIGNED_URL_TTL: Duration = Duration::from_secs(5 * 60);
const HANDLE_CALLER_AGENT_HEADER: &str = "x-talon-agent";
const HANDLE_CALLER_SESSION_HEADER: &str = "x-talon-session-id";

#[derive(Debug, Clone, Default)]
struct HandleCaller {
    agent: String,
    session_id: String,
}

#[derive(Debug, Clone)]
enum FileServiceError {
    NotFound(String),
    PermissionDenied(String),
    InvalidArgument(String),
}

impl FileServiceError {
    fn into_status(self) -> Status {
        match self {
            Self::NotFound(message) => Status::not_found(message),
            Self::PermissionDenied(message) => Status::permission_denied(message),
            Self::InvalidArgument(message) => Status::invalid_argument(message),
        }
    }
}

impl fmt::Display for FileServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message)
            | Self::PermissionDenied(message)
            | Self::InvalidArgument(message) => f.write_str(message),
        }
    }
}

impl StdError for FileServiceError {}

fn not_found(message: impl Into<String>) -> anyhow::Error {
    FileServiceError::NotFound(message.into()).into()
}

fn permission_denied(message: impl Into<String>) -> anyhow::Error {
    FileServiceError::PermissionDenied(message.into()).into()
}

fn invalid_argument(message: impl Into<String>) -> anyhow::Error {
    FileServiceError::InvalidArgument(message.into()).into()
}

impl GrpcGatewayHandler {
    async fn create_file_impl(
        &self,
        namespace: &str,
        path: &str,
        media_type: &str,
        purpose: i32,
        index_policy: i32,
        retention: i32,
        content: &[u8],
    ) -> Result<(resources_proto::File, String)> {
        let path = normalize_logical_path(path)?;
        validate_file_spec(purpose, index_policy, retention)?;
        let media_type = normalize_media_type(media_type)?;
        let existing = self.find_file_by_path(namespace, &path).await?;
        let is_new_file = existing.is_none();
        let existing_uid = existing
            .as_ref()
            .and_then(|file| file.metadata.as_ref())
            .map(|meta| meta.uid.as_str())
            .filter(|uid| !uid.is_empty());
        let name = existing
            .as_ref()
            .map(|file| file.name().to_string())
            .unwrap_or_else(|| file_resource_name_for_path(&path));
        let status = existing
            .as_ref()
            .and_then(|file| file.status.clone())
            .unwrap_or_default();
        let previous_object_key = status.object_ref.as_ref().map(|object| object.key.clone());
        let labels = file_labels(purpose, index_policy, retention);
        let spec = resources_proto::FileSpec {
            path: path.clone(),
            media_type: media_type.clone(),
            purpose,
            index_policy,
            retention,
        };
        let prewritten_object_ref = match existing_uid {
            Some(uid) => Some(
                self.write_file_cas_object(namespace, uid, &path, &media_type, content)
                    .await?,
            ),
            None => None,
        };
        let resource =
            resource_model::file_resource(namespace.to_string(), name, spec, status, labels);
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let mut resource = match store.upsert(namespace, resource).await {
            Ok(resource) => resource,
            Err(error) => {
                if let Some(object_ref) = prewritten_object_ref.as_ref() {
                    if previous_object_key.as_ref() != Some(&object_ref.key) {
                        if let Err(cleanup_error) = CasStore::new(self.gateway.objects.clone())
                            .delete_object(&object_ref.key)
                            .await
                        {
                            tracing::warn!(
                                error = %cleanup_error,
                                object_key = %object_ref.key,
                                "failed to clean up prewritten File CAS object after resource upsert failure"
                            );
                        }
                    }
                }
                return Err(error);
            }
        };
        let uid = resource
            .metadata
            .as_ref()
            .map(|meta| meta.uid.as_str())
            .filter(|uid| !uid.is_empty())
            .ok_or_else(|| anyhow!("File resource uid missing after upsert"))?;
        let resource_name = file_name(&resource)?.to_string();
        let object_ref = match prewritten_object_ref {
            Some(object_ref) => object_ref,
            None => match self
                .write_file_cas_object(namespace, uid, &path, &media_type, content)
                .await
            {
                Ok(object_ref) => object_ref,
                Err(error) => {
                    if is_new_file {
                        if let Err(cleanup_error) =
                            store.delete(namespace, "File", &resource_name).await
                        {
                            tracing::warn!(
                                error = %cleanup_error,
                                namespace = %namespace,
                                file = %resource_name,
                                "failed to roll back File resource after CAS write failure"
                            );
                        }
                    }
                    return Err(error);
                }
            },
        };
        let new_object_key = object_ref.key.clone();
        let updated_at = chrono::Utc::now().timestamp_micros();
        let status = resources_proto::FileStatus {
            observed_generation: resource
                .metadata
                .as_ref()
                .map(|meta| meta.generation)
                .unwrap_or_default(),
            phase: "Ready".to_string(),
            conditions: Vec::new(),
            object_ref: Some(file_object_ref(object_ref)),
            updated_at,
        };
        resource.status = Some(resources_proto::ResourceStatus {
            kind: Some(resources_proto::resource_status::Kind::File(status)),
        });
        let status = resource.status.clone().unwrap();
        let resource = match store
            .patch_status(namespace, "File", &resource_name, None, status)
            .await
        {
            Ok(resource) => resource,
            Err(error) => {
                if is_new_file && previous_object_key.as_ref() != Some(&new_object_key) {
                    let cas = CasStore::new(self.gateway.objects.clone());
                    if let Err(cleanup_error) = cas.delete_object(&new_object_key).await {
                        tracing::warn!(
                            error = %cleanup_error,
                            object_key = %new_object_key,
                            namespace = %namespace,
                            file = %resource_name,
                            "failed to clean up File CAS object after status patch failure"
                        );
                    }
                } else if !is_new_file && previous_object_key.as_ref() != Some(&new_object_key) {
                    tracing::warn!(
                        error = %error,
                        object_key = %new_object_key,
                        namespace = %namespace,
                        file = %resource_name,
                        "leaving File CAS object in place after status patch failure because status may have committed"
                    );
                }
                if is_new_file {
                    if let Err(cleanup_error) =
                        store.delete(namespace, "File", &resource_name).await
                    {
                        tracing::warn!(
                            error = %cleanup_error,
                            namespace = %namespace,
                            file = %resource_name,
                            "failed to roll back File resource after status patch failure"
                        );
                    }
                }
                return Err(error);
            }
        };
        if let Err(error) = self
            .write_latest_file_object(namespace, &path, &media_type, content)
            .await
        {
            tracing::warn!(
                error = %error,
                namespace = %namespace,
                file = %resource_name,
                path = %path,
                "failed to materialize latest File object"
            );
        }
        if let Some(previous_object_key) = previous_object_key {
            if previous_object_key != new_object_key {
                if let Err(error) = CasStore::new(self.gateway.objects.clone())
                    .delete_object(&previous_object_key)
                    .await
                {
                    tracing::warn!(
                        error = %error,
                        namespace = %namespace,
                        file = %resource_name,
                        object_key = %previous_object_key,
                        "failed to delete previous File CAS object"
                    );
                }
            }
        }
        let file = file_from_resource(resource)?;
        let handle = self
            .mint_handle(
                namespace,
                HANDLE_KIND_FILE,
                file.name(),
                "",
                "",
                &[OP_READ, OP_METADATA, OP_WRITE, OP_DELETE],
                "",
                "",
                default_handle_expiry(),
            )
            .await?;
        Ok((file, handle))
    }

    async fn write_file_cas_object(
        &self,
        namespace: &str,
        file_uid: &str,
        path: &str,
        media_type: &str,
        content: &[u8],
    ) -> Result<data_proto::ObjectRef> {
        CasStore::new(self.gateway.objects.clone())
            .put_file(namespace, file_uid, path, content, media_type)
            .await
    }

    async fn write_latest_file_object(
        &self,
        namespace: &str,
        path: &str,
        media_type: &str,
        content: &[u8],
    ) -> Result<data_proto::ObjectRef> {
        CasStore::new(self.gateway.objects.clone())
            .put_latest_file(namespace, path, content, media_type)
            .await
    }

    async fn find_file_by_path(
        &self,
        namespace: &str,
        path: &str,
    ) -> Result<Option<resources_proto::File>> {
        let name = file_resource_name_for_path(path);
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        if let Some(resource) = store.get(namespace, "File", &name).await? {
            let file = file_from_resource(resource)?;
            if file.spec.as_ref().map(|spec| spec.path.as_str()) == Some(path) {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }

    async fn get_file_by_ref(
        &self,
        reference: Option<proto::FileRef>,
        operation: &str,
        caller: HandleCaller,
    ) -> Result<resources_proto::File> {
        let reference = reference.ok_or_else(|| invalid_argument("file reference is required"))?;
        if !reference.handle.trim().is_empty() {
            let grant = self
                .resolve_handle(&reference.handle, operation, caller)
                .await?;
            if grant.kind != HANDLE_KIND_FILE {
                return Err(invalid_argument("handle is not a file handle"));
            }
            return self
                .get_file_by_name(&grant.namespace, &grant.target_id)
                .await;
        }
        if !reference.name.trim().is_empty() {
            return self
                .get_file_by_name(&reference.namespace, &reference.name)
                .await;
        }
        if !reference.path.trim().is_empty() {
            let path = normalize_logical_path(&reference.path)?;
            return self
                .find_file_by_path(&reference.namespace, &path)
                .await?
                .ok_or_else(|| not_found(format!("File '{}' not found", path)));
        }
        Err(invalid_argument(
            "file reference must include handle, name, or path",
        ))
    }

    async fn get_file_by_name(&self, namespace: &str, name: &str) -> Result<resources_proto::File> {
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let resource = store
            .get(namespace, "File", name)
            .await?
            .ok_or_else(|| not_found(format!("File '{}' not found", name)))?;
        file_from_resource(resource)
    }

    async fn mint_handle(
        &self,
        namespace: &str,
        kind: &str,
        target_id: &str,
        agent: &str,
        session_id: &str,
        operations: &[&str],
        audience_agent: &str,
        audience_session_id: &str,
        expires_at: i64,
    ) -> Result<String> {
        let id = crate::control::uuid::unique_name("hnd");
        let grant = data_proto::HandleGrant {
            id: id.clone(),
            namespace: namespace.to_string(),
            kind: kind.to_string(),
            target_id: target_id.to_string(),
            agent: agent.to_string(),
            session_id: session_id.to_string(),
            operations: operations.iter().map(|op| op.to_string()).collect(),
            audience_agent: audience_agent.to_string(),
            audience_session_id: audience_session_id.to_string(),
            expires_at,
            created_at: chrono::Utc::now().timestamp_micros(),
        };
        self.gateway
            .kv
            .set_msg(&keys::handle_grant(namespace, &id), &grant)
            .await?;
        Ok(format!("talon-handle:{}/{}", encoded_ns(namespace), id))
    }

    async fn resolve_handle(
        &self,
        handle: &str,
        operation: &str,
        caller: HandleCaller,
    ) -> Result<data_proto::HandleGrant> {
        let (namespace, id) = handle_namespace_and_id(handle)?;
        let grant = self
            .gateway
            .kv
            .get_msg::<data_proto::HandleGrant>(&keys::handle_grant(&namespace, &id))
            .await?
            .ok_or_else(|| not_found(format!("handle '{}' not found", handle)))?;
        if grant.expires_at > 0 && grant.expires_at < chrono::Utc::now().timestamp_micros() {
            return Err(permission_denied(format!("handle '{}' is expired", handle)));
        }
        if !grant.operations.iter().any(|op| op == operation) {
            return Err(permission_denied(format!(
                "handle '{}' does not allow '{}'",
                handle, operation
            )));
        }
        validate_handle_audience(&grant, caller)?;
        Ok(grant)
    }

    async fn read_object_content_or_signed_url(
        &self,
        key: &str,
        size_bytes: u64,
    ) -> std::result::Result<(Vec<u8>, String, i64), Status> {
        let cas = CasStore::new(self.gateway.objects.clone());
        let signed = cas
            .signed_get_url(key, FILE_SIGNED_URL_TTL)
            .await
            .map_err(|err| {
                Status::internal(format!("Failed to sign object download URL: {err}"))
            })?;
        if let Some(signed) = signed {
            return Ok((Vec::new(), signed.url, signed.expires_at_unix_seconds));
        }

        ensure_unary_object_size(size_bytes)?;
        let object = cas
            .get_object_decoded(key)
            .await
            .map_err(to_status)?
            .ok_or_else(|| Status::not_found("Object not found"))?;
        Ok((object.bytes, String::new(), 0))
    }
}

#[tonic::async_trait]
impl proto::file_service_server::FileService for GrpcGatewayHandler {
    async fn create_file(
        &self,
        req: Request<proto::CreateFileRequest>,
    ) -> std::result::Result<Response<proto::FileResponse>, Status> {
        let namespace = req.get_ref().namespace.clone();
        require_auth!(self, req, &namespace);
        let req = req.into_inner();
        ensure_unary_content_len(req.content.len())?;
        let (file, file_handle) = self
            .create_file_impl(
                &req.namespace,
                &req.path,
                &req.media_type,
                req.purpose,
                req.index_policy,
                req.retention,
                &req.content,
            )
            .await
            .map_err(to_status)?;
        Ok(Response::new(proto::FileResponse {
            file: Some(file),
            file_handle,
        }))
    }

    async fn read_file(
        &self,
        req: Request<proto::ReadFileRequest>,
    ) -> std::result::Result<Response<proto::ReadFileResponse>, Status> {
        require_direct_file_ref_auth(self, &req, OP_READ)?;
        let caller = handle_caller_from_request(self, &req);
        let req = req.into_inner();
        let file = self
            .get_file_by_ref(req.file, OP_READ, caller)
            .await
            .map_err(to_status)?;
        let object_ref = file
            .status
            .as_ref()
            .and_then(|status| status.object_ref.as_ref())
            .ok_or_else(|| Status::failed_precondition("File has no objectRef"))?;
        let (content, signed_url, signed_url_expires_at_unix_seconds) = self
            .read_object_content_or_signed_url(&object_ref.key, object_ref.size_bytes)
            .await?;
        Ok(Response::new(proto::ReadFileResponse {
            file: Some(file),
            content,
            signed_url,
            signed_url_expires_at_unix_seconds,
        }))
    }

    async fn update_file(
        &self,
        req: Request<proto::UpdateFileRequest>,
    ) -> std::result::Result<Response<proto::FileResponse>, Status> {
        require_direct_file_ref_auth(self, &req, OP_WRITE)?;
        let caller = handle_caller_from_request(self, &req);
        let req = req.into_inner();
        ensure_unary_content_len(req.content.len())?;
        let file = self
            .get_file_by_ref(req.file, OP_WRITE, caller)
            .await
            .map_err(to_status)?;
        let namespace = file.namespace().to_string();
        let name = file.name().to_string();
        let spec = file
            .spec
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("File spec missing"))?;
        let media_type = non_empty(&req.media_type).unwrap_or(&spec.media_type);
        let (updated, handle) = self
            .create_file_impl(
                &namespace,
                &spec.path,
                media_type,
                spec.purpose,
                spec.index_policy,
                spec.retention,
                &req.content,
            )
            .await
            .map_err(to_status)?;
        if updated.name() != name {
            return Err(Status::internal("updated File name changed unexpectedly"));
        }
        Ok(Response::new(proto::FileResponse {
            file: Some(updated),
            file_handle: handle,
        }))
    }

    async fn get_file_metadata(
        &self,
        req: Request<proto::GetFileMetadataRequest>,
    ) -> std::result::Result<Response<proto::FileResponse>, Status> {
        require_direct_file_ref_auth(self, &req, OP_METADATA)?;
        let caller = handle_caller_from_request(self, &req);
        let file = self
            .get_file_by_ref(req.into_inner().file, OP_METADATA, caller)
            .await
            .map_err(to_status)?;
        Ok(Response::new(proto::FileResponse {
            file: Some(file),
            file_handle: String::new(),
        }))
    }

    async fn list_files(
        &self,
        req: Request<proto::ListFilesRequest>,
    ) -> std::result::Result<Response<proto::ListFilesResponse>, Status> {
        let namespace = req.get_ref().namespace.clone();
        require_auth!(read, self, req, &namespace);
        let req = req.into_inner();
        let prefix = normalize_prefix(&req.prefix).map_err(to_status)?;
        let limit = (req.limit as usize).clamp(1, 200);
        let mut before_name = normalize_resource_name_cursor(&req.page_token).map_err(to_status)?;
        let list = keys::ResourceParent::root(&req.namespace).list(Some("File"));
        let mut files = Vec::with_capacity(limit);
        let mut scanned_pages = 0usize;
        let mut next_page_token = String::new();
        while files.len() < limit {
            if scanned_pages >= MAX_FILE_LIST_SCAN_PAGES {
                next_page_token = before_name.unwrap_or_default();
                break;
            }
            scanned_pages += 1;
            let entries = self
                .gateway
                .kv
                .list_entries_page(&list, before_name.as_deref(), FILE_LIST_SCAN_PAGE_SIZE)
                .await
                .map_err(to_status)?;
            if entries.is_empty() {
                break;
            }
            for (key, value) in entries {
                before_name = Some(key.name.clone());
                let file = match ResourceStore::decode_stored_resource(&key.kind, value.as_slice())
                    .and_then(file_from_resource)
                {
                    Ok(file) => file,
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            resource = %key.canonical(),
                            "failed to decode File resource during list_files"
                        );
                        continue;
                    }
                };
                let Some(spec) = file.spec.as_ref() else {
                    continue;
                };
                if !path_matches_prefix(&spec.path, &prefix)
                    || (req.purpose != 0 && spec.purpose != req.purpose)
                    || (req.index_policy != 0 && spec.index_policy != req.index_policy)
                {
                    continue;
                }
                files.push(file);
                if files.len() == limit {
                    next_page_token = before_name.clone().unwrap_or_default();
                    break;
                }
            }
            if before_name.is_none() {
                break;
            }
        }
        Ok(Response::new(proto::ListFilesResponse {
            files,
            next_page_token,
        }))
    }

    async fn delete_file(
        &self,
        req: Request<proto::DeleteFileRequest>,
    ) -> std::result::Result<Response<proto::DeleteFileResponse>, Status> {
        require_direct_file_ref_auth(self, &req, OP_DELETE)?;
        let caller = handle_caller_from_request(self, &req);
        let file = self
            .get_file_by_ref(req.into_inner().file, OP_DELETE, caller)
            .await
            .map_err(to_status)?;
        let latest_key = file
            .spec
            .as_ref()
            .map(|spec| latest_file_object_key(file.namespace(), &spec.path));
        let object_key = file
            .status
            .as_ref()
            .and_then(|status| status.object_ref.as_ref())
            .map(|object| object.key.clone());
        let store = ResourceStore::new(self.gateway.kv.clone(), self.gateway.pubsub.clone());
        let success = store
            .delete(file.namespace(), "File", file.name())
            .await
            .map_err(to_status)?;
        if success {
            let cas = CasStore::new(self.gateway.objects.clone());
            if let Some(key) = latest_key {
                if let Err(error) = cas.delete_object(&key).await {
                    tracing::warn!(
                        error = %error,
                        object_key = %key,
                        "failed to delete latest File object"
                    );
                }
            }
            if let Some(key) = object_key {
                if let Err(error) = cas.delete_object(&key).await {
                    tracing::warn!(
                        error = %error,
                        object_key = %key,
                        "failed to delete File CAS object"
                    );
                }
            }
        }
        Ok(Response::new(proto::DeleteFileResponse { success }))
    }

    async fn promote_artifact(
        &self,
        req: Request<proto::PromoteArtifactRequest>,
    ) -> std::result::Result<Response<proto::FileResponse>, Status> {
        let grant = self
            .resolve_handle(
                &req.get_ref().artifact_handle,
                OP_PROMOTE,
                handle_caller_from_request(self, &req),
            )
            .await
            .map_err(to_status)?;
        require_auth!(self, req, &grant.namespace);
        if grant.kind != HANDLE_KIND_ARTIFACT {
            return Err(Status::invalid_argument("handle is not an artifact handle"));
        }
        let req = req.into_inner();
        let artifact = load_artifact(self, &grant).await.map_err(to_status)?;
        let object_ref = artifact
            .object_ref
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Artifact has no objectRef"))?;
        ensure_unary_object_size(object_ref.size_bytes)?;
        let object = CasStore::new(self.gateway.objects.clone())
            .get_object_decoded(&object_ref.key)
            .await
            .map_err(to_status)?
            .ok_or_else(|| Status::not_found("Artifact object not found"))?;
        let media_type = non_empty(&req.media_type)
            .or_else(|| non_empty(&artifact.media_type))
            .unwrap_or("application/octet-stream");
        let purpose = if req.purpose == 0 {
            resources_proto::FilePurpose::Artifact as i32
        } else {
            req.purpose
        };
        let index_policy = if req.index_policy == 0 {
            resources_proto::FileIndexPolicy::None as i32
        } else {
            req.index_policy
        };
        let retention = if req.retention == 0 {
            resources_proto::FileRetention::Retained as i32
        } else {
            req.retention
        };
        let (file, file_handle) = self
            .create_file_impl(
                &grant.namespace,
                &req.target_path,
                media_type,
                purpose,
                index_policy,
                retention,
                &object.bytes,
            )
            .await
            .map_err(to_status)?;
        Ok(Response::new(proto::FileResponse {
            file: Some(file),
            file_handle,
        }))
    }
}

#[tonic::async_trait]
impl proto::artifact_service_server::ArtifactService for GrpcGatewayHandler {
    async fn read_artifact(
        &self,
        req: Request<proto::ReadArtifactRequest>,
    ) -> std::result::Result<Response<proto::ReadArtifactResponse>, Status> {
        let grant = self
            .resolve_handle(
                &req.get_ref().artifact_handle,
                OP_READ,
                handle_caller_from_request(self, &req),
            )
            .await
            .map_err(to_status)?;
        if grant.kind != HANDLE_KIND_ARTIFACT {
            return Err(Status::invalid_argument("handle is not an artifact handle"));
        }
        let artifact = load_artifact(self, &grant).await.map_err(to_status)?;
        let object_ref = artifact
            .object_ref
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Artifact has no objectRef"))?;
        let (content, signed_url, signed_url_expires_at_unix_seconds) = self
            .read_object_content_or_signed_url(&object_ref.key, object_ref.size_bytes)
            .await?;
        Ok(Response::new(proto::ReadArtifactResponse {
            artifact: Some(artifact),
            content,
            signed_url,
            signed_url_expires_at_unix_seconds,
        }))
    }

    async fn get_artifact_metadata(
        &self,
        req: Request<proto::GetArtifactMetadataRequest>,
    ) -> std::result::Result<Response<proto::ArtifactResponse>, Status> {
        let grant = self
            .resolve_handle(
                &req.get_ref().artifact_handle,
                OP_METADATA,
                handle_caller_from_request(self, &req),
            )
            .await
            .map_err(to_status)?;
        if grant.kind != HANDLE_KIND_ARTIFACT {
            return Err(Status::invalid_argument("handle is not an artifact handle"));
        }
        let artifact = load_artifact(self, &grant).await.map_err(to_status)?;
        Ok(Response::new(proto::ArtifactResponse {
            artifact: Some(artifact),
            artifact_handle: String::new(),
        }))
    }

    async fn list_artifacts(
        &self,
        req: Request<proto::ListArtifactsRequest>,
    ) -> std::result::Result<Response<proto::ListArtifactsResponse>, Status> {
        let namespace = req.get_ref().namespace.clone();
        require_auth!(read, self, req, &namespace);
        let req = req.into_inner();
        let prefix = normalize_prefix(&req.prefix).map_err(to_status)?;
        let limit = (req.limit as usize).clamp(1, 200);
        let mut before_name = normalize_resource_name_cursor(&req.page_token).map_err(to_status)?;
        let list = keys::artifact_prefix(&req.namespace, &req.agent, &req.session_id);
        let mut artifacts = Vec::with_capacity(limit);
        let mut scanned_pages = 0usize;
        let mut next_page_token = String::new();
        while artifacts.len() < limit {
            if scanned_pages >= MAX_FILE_LIST_SCAN_PAGES {
                next_page_token = before_name.unwrap_or_default();
                break;
            }
            scanned_pages += 1;
            let entries = self
                .gateway
                .kv
                .list_entries_page(&list, before_name.as_deref(), FILE_LIST_SCAN_PAGE_SIZE)
                .await
                .map_err(to_status)?;
            if entries.is_empty() {
                break;
            }
            for (key, bytes) in entries {
                before_name = Some(key.name.clone());
                let Ok(artifact) = data_proto::Artifact::decode(bytes.as_slice()) else {
                    tracing::warn!(
                        resource = %key.canonical(),
                        "failed to decode Artifact during list_artifacts"
                    );
                    continue;
                };
                if !path_matches_prefix(&artifact.path, &prefix) {
                    continue;
                }
                artifacts.push(artifact);
                if artifacts.len() == limit {
                    next_page_token = before_name.clone().unwrap_or_default();
                    break;
                }
            }
        }
        Ok(Response::new(proto::ListArtifactsResponse {
            artifacts,
            next_page_token,
        }))
    }

    async fn grant_artifact(
        &self,
        req: Request<proto::GrantArtifactRequest>,
    ) -> std::result::Result<Response<proto::ArtifactHandleResponse>, Status> {
        let caller = handle_caller_from_request(self, &req);
        let req = req.into_inner();
        let grant = self
            .resolve_handle(&req.artifact_handle, OP_READ, caller)
            .await
            .map_err(to_status)?;
        if grant.kind != HANDLE_KIND_ARTIFACT {
            return Err(Status::invalid_argument("handle is not an artifact handle"));
        }
        let operations = if req.operations.is_empty() {
            vec![OP_READ, OP_METADATA]
        } else {
            req.operations
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        };
        validate_artifact_operations(&operations).map_err(to_status)?;
        for operation in &operations {
            if !grant.operations.iter().any(|allowed| allowed == operation) {
                return Err(Status::permission_denied(format!(
                    "source handle does not allow '{}'",
                    operation
                )));
            }
        }
        let ttl = if req.ttl_seconds <= 0 {
            default_handle_expiry()
        } else {
            let ttl_micros = req.ttl_seconds.min(MAX_HANDLE_TTL_SECONDS) * 1_000_000;
            chrono::Utc::now()
                .timestamp_micros()
                .saturating_add(ttl_micros)
        };
        let handle = self
            .mint_handle(
                &grant.namespace,
                HANDLE_KIND_ARTIFACT,
                &grant.target_id,
                &grant.agent,
                &grant.session_id,
                &operations,
                &req.target_agent,
                &req.target_session_id,
                ttl,
            )
            .await
            .map_err(to_status)?;
        Ok(Response::new(proto::ArtifactHandleResponse {
            artifact_handle: handle,
        }))
    }
}

fn require_direct_file_ref_auth<T>(
    handler: &GrpcGatewayHandler,
    req: &Request<T>,
    operation: &str,
) -> std::result::Result<(), Status>
where
    T: FileRefRequest,
{
    let Some(reference) = req.get_ref().file_ref() else {
        return Err(Status::invalid_argument("file reference is required"));
    };
    if !reference.handle.trim().is_empty() {
        return Ok(());
    }
    if reference.namespace.trim().is_empty() {
        return Err(Status::invalid_argument(
            "namespace is required without a file handle",
        ));
    }
    if let Some(auth_config) = &handler.gateway.auth_config {
        let auth_operation = match operation {
            OP_READ | OP_METADATA => crate::gateway::auth::AuthzOperation::Read,
            _ => crate::gateway::auth::AuthzOperation::ReadWrite,
        };
        crate::gateway::auth::check_auth_for_operation(
            req.metadata(),
            auth_config,
            auth_operation,
            &reference.namespace,
            None,
            None,
        )?;
    }
    Ok(())
}

fn handle_caller_from_request<T>(handler: &GrpcGatewayHandler, req: &Request<T>) -> HandleCaller {
    if handler.gateway.auth_config.is_some() {
        return req
            .extensions()
            .get::<Claims>()
            .map(|claims| HandleCaller {
                agent: claims.agent.clone().unwrap_or_default(),
                session_id: claims.session.clone().unwrap_or_default(),
            })
            .unwrap_or_default();
    }
    handle_caller_from_metadata(req.metadata())
}

fn handle_caller_from_metadata(metadata: &MetadataMap) -> HandleCaller {
    HandleCaller {
        agent: metadata_ascii(metadata, HANDLE_CALLER_AGENT_HEADER).unwrap_or_default(),
        session_id: metadata_ascii(metadata, HANDLE_CALLER_SESSION_HEADER).unwrap_or_default(),
    }
}

fn metadata_ascii(metadata: &MetadataMap, key: &'static str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_handle_audience(grant: &data_proto::HandleGrant, caller: HandleCaller) -> Result<()> {
    if !grant.audience_agent.trim().is_empty() && grant.audience_agent != caller.agent {
        if caller.agent.trim().is_empty() {
            return Err(permission_denied("handle requires caller agent identity"));
        }
        return Err(permission_denied(
            "handle audience agent does not match caller",
        ));
    }
    if !grant.audience_session_id.trim().is_empty()
        && grant.audience_session_id != caller.session_id
    {
        if caller.session_id.trim().is_empty() {
            return Err(permission_denied("handle requires caller session identity"));
        }
        return Err(permission_denied(
            "handle audience session does not match caller",
        ));
    }
    Ok(())
}

trait FileRefRequest {
    fn file_ref(&self) -> Option<&proto::FileRef>;
}

impl FileRefRequest for proto::ReadFileRequest {
    fn file_ref(&self) -> Option<&proto::FileRef> {
        self.file.as_ref()
    }
}

impl FileRefRequest for proto::UpdateFileRequest {
    fn file_ref(&self) -> Option<&proto::FileRef> {
        self.file.as_ref()
    }
}

impl FileRefRequest for proto::GetFileMetadataRequest {
    fn file_ref(&self) -> Option<&proto::FileRef> {
        self.file.as_ref()
    }
}

impl FileRefRequest for proto::DeleteFileRequest {
    fn file_ref(&self) -> Option<&proto::FileRef> {
        self.file.as_ref()
    }
}

fn validate_artifact_operations(operations: &[&str]) -> Result<()> {
    for operation in operations {
        match *operation {
            OP_READ | OP_METADATA | OP_PROMOTE => {}
            other => {
                return Err(invalid_argument(format!(
                    "unsupported artifact operation '{}'",
                    other
                )));
            }
        }
    }
    Ok(())
}

async fn load_artifact(
    handler: &GrpcGatewayHandler,
    grant: &data_proto::HandleGrant,
) -> Result<data_proto::Artifact> {
    handler
        .gateway
        .kv
        .get_msg::<data_proto::Artifact>(&keys::artifact(
            &grant.namespace,
            &grant.agent,
            &grant.session_id,
            &grant.target_id,
        ))
        .await?
        .ok_or_else(|| not_found(format!("Artifact '{}' not found", grant.target_id)))
}

fn file_from_resource(resource: resources_proto::Resource) -> Result<resources_proto::File> {
    let spec = resource.spec.and_then(|spec| match spec.kind {
        Some(resources_proto::resource_spec::Kind::File(spec)) => Some(spec),
        _ => None,
    });
    let status = resource.status.and_then(|status| match status.kind {
        Some(resources_proto::resource_status::Kind::File(status)) => Some(status),
        _ => None,
    });
    Ok(resources_proto::File {
        metadata: resource.metadata,
        spec,
        status,
    })
}

fn file_object_ref(object_ref: data_proto::ObjectRef) -> resources_proto::FileObjectRef {
    resources_proto::FileObjectRef {
        key: object_ref.key,
        media_type: object_ref.media_type,
        size_bytes: object_ref.size_bytes,
        sha256: object_ref.sha256,
        filename: object_ref.filename,
        metadata: object_ref.metadata,
    }
}

trait FileExt {
    fn name(&self) -> &str;
    fn namespace(&self) -> &str;
}

impl FileExt for resources_proto::File {
    fn name(&self) -> &str {
        self.metadata
            .as_ref()
            .map(|meta| meta.name.as_str())
            .unwrap_or_default()
    }

    fn namespace(&self) -> &str {
        self.metadata
            .as_ref()
            .map(|meta| meta.namespace.as_str())
            .unwrap_or_default()
    }
}

fn file_name(resource: &resources_proto::Resource) -> Result<&str> {
    resource
        .metadata
        .as_ref()
        .map(|meta| meta.name.as_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("File resource name missing"))
}

fn validate_file_spec(purpose: i32, index_policy: i32, retention: i32) -> Result<()> {
    if resources_proto::FilePurpose::try_from(purpose).is_err() || purpose == 0 {
        return Err(invalid_argument("File purpose is required"));
    }
    if resources_proto::FileIndexPolicy::try_from(index_policy).is_err() || index_policy == 0 {
        return Err(invalid_argument("File indexPolicy is required"));
    }
    if resources_proto::FileRetention::try_from(retention).is_err() || retention == 0 {
        return Err(invalid_argument("File retention is required"));
    }
    Ok(())
}

fn normalize_logical_path(path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(invalid_argument("path is required"));
    }
    if !path.starts_with('/') {
        return Err(invalid_argument("path must be absolute"));
    }
    if path.contains("//") {
        return Err(invalid_argument("path is not normalized"));
    }
    if path.contains('\0')
        || path.contains('\\')
        || path.contains("/./")
        || path.ends_with("/.")
        || path.chars().any(char::is_control)
    {
        return Err(invalid_argument("path is not normalized"));
    }
    let normalized = path.trim_end_matches('/');
    if normalized.is_empty() {
        return Err(invalid_argument("path cannot be root '/'"));
    }
    if normalized
        .trim_start_matches('/')
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(invalid_argument("path is not normalized"));
    }
    Ok(normalized.to_string())
}

fn normalize_prefix(prefix: &str) -> Result<String> {
    if prefix.trim().is_empty() {
        return Ok(String::new());
    }
    normalize_logical_path(prefix)
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn normalize_media_type(value: &str) -> Result<String> {
    let value = non_empty(value).unwrap_or("application/octet-stream");
    if value.len() > 255 || value.chars().any(char::is_control) {
        return Err(invalid_argument("mediaType is invalid"));
    }
    let essence = value
        .split_once(';')
        .map(|(essence, _)| essence)
        .unwrap_or(value)
        .trim();
    let Some((type_, subtype)) = essence.split_once('/') else {
        return Err(invalid_argument("mediaType must be type/subtype"));
    };
    if !is_media_type_token(type_) || !is_media_type_token(subtype) {
        return Err(invalid_argument("mediaType is invalid"));
    }
    Ok(value.to_string())
}

fn is_media_type_token(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '!' | '#' | '$' | '&' | '^' | '_' | '.' | '+' | '-')
        })
}

fn normalize_resource_name_cursor(page_token: &str) -> Result<Option<String>> {
    let page_token = page_token.trim();
    if page_token.is_empty() {
        return Ok(None);
    }
    if page_token.contains('/') || page_token.contains('\0') {
        return Err(invalid_argument("page token is invalid"));
    }
    Ok(Some(page_token.to_string()))
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn file_labels(purpose: i32, index_policy: i32, retention: i32) -> HashMap<String, String> {
    HashMap::from([
        (
            "talon.impalasys.com/purpose".to_string(),
            purpose_label(purpose).to_string(),
        ),
        (
            "talon.impalasys.com/index-policy".to_string(),
            index_policy_label(index_policy).to_string(),
        ),
        (
            "talon.impalasys.com/retention".to_string(),
            retention_label(retention).to_string(),
        ),
    ])
}

fn purpose_label(value: i32) -> &'static str {
    match resources_proto::FilePurpose::try_from(value).ok() {
        Some(resources_proto::FilePurpose::Memory) => "memory",
        Some(resources_proto::FilePurpose::Artifact) => "artifact",
        _ => "unspecified",
    }
}

fn index_policy_label(value: i32) -> &'static str {
    match resources_proto::FileIndexPolicy::try_from(value).ok() {
        Some(resources_proto::FileIndexPolicy::None) => "none",
        Some(resources_proto::FileIndexPolicy::Search) => "search",
        Some(resources_proto::FileIndexPolicy::Retrieval) => "retrieval",
        _ => "unspecified",
    }
}

fn retention_label(value: i32) -> &'static str {
    match resources_proto::FileRetention::try_from(value).ok() {
        Some(resources_proto::FileRetention::Retained) => "retained",
        _ => "unspecified",
    }
}

fn encoded_ns(namespace: &str) -> String {
    urlencoding::encode(namespace).into_owned()
}

fn handle_namespace_and_id(handle: &str) -> Result<(String, String)> {
    let handle = handle.trim();
    let Some(rest) = handle.strip_prefix("talon-handle:") else {
        return Err(invalid_argument("handle must start with 'talon-handle:'"));
    };
    let Some((namespace, id)) = rest.split_once('/') else {
        return Err(invalid_argument(
            "handle must be 'talon-handle:<namespace>/<id>'",
        ));
    };
    let namespace = urlencoding::decode(namespace)
        .map(|value| value.into_owned())
        .context("failed to decode handle namespace")?;
    Ok((namespace, id.to_string()))
}

fn default_handle_expiry() -> i64 {
    chrono::Utc::now().timestamp_micros() + 24 * 60 * 60 * 1_000_000
}

fn ensure_unary_content_len(size_bytes: usize) -> std::result::Result<(), Status> {
    if size_bytes > MAX_UNARY_FILE_CONTENT_BYTES {
        return Err(Status::resource_exhausted(format!(
            "file content is {} bytes; unary File/Artifact RPCs are capped at {} bytes",
            size_bytes, MAX_UNARY_FILE_CONTENT_BYTES
        )));
    }
    Ok(())
}

fn ensure_unary_object_size(size_bytes: u64) -> std::result::Result<(), Status> {
    if size_bytes > MAX_UNARY_FILE_CONTENT_BYTES as u64 {
        return Err(Status::resource_exhausted(format!(
            "file content is {} bytes; unary File/Artifact RPCs are capped at {} bytes",
            size_bytes, MAX_UNARY_FILE_CONTENT_BYTES
        )));
    }
    Ok(())
}

fn to_status(error: anyhow::Error) -> Status {
    if let Some(status) = error.downcast_ref::<Status>() {
        return status.clone();
    }
    if let Some(error) = error.downcast_ref::<FileServiceError>() {
        return error.clone().into_status();
    }
    tracing::error!(error = %error, "FileService internal error");
    Status::internal("An internal error occurred")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::object_store::{
        ObjectMetadata, ObjectStore, SignedObjectUrl, StoredObject,
    };
    use crate::control::ControlPlane;
    use crate::gateway::auth::AuthConfig;
    use crate::gateway::Gateway;
    use crate::test_support::{MockKvStore, RecordingPubSub};
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;

    struct SignedUrlOnlyObjectStore;

    #[async_trait]
    impl ObjectStore for SignedUrlOnlyObjectStore {
        async fn put(
            &self,
            key: &str,
            _bytes: &[u8],
            metadata: ObjectMetadata,
        ) -> Result<data_proto::ObjectRef> {
            Ok(data_proto::ObjectRef {
                key: key.to_string(),
                media_type: metadata.media_type,
                size_bytes: metadata.size_bytes,
                sha256: metadata.sha256,
                filename: metadata.filename,
                metadata: metadata.metadata,
                content_encoding: metadata.content_encoding,
            })
        }

        async fn get(&self, _key: &str) -> Result<Option<StoredObject>> {
            panic!("signed URL reads should not fetch bytes")
        }

        async fn head(&self, _key: &str) -> Result<Option<ObjectMetadata>> {
            Ok(None)
        }

        async fn delete(&self, _key: &str) -> Result<()> {
            Ok(())
        }

        async fn signed_get_url(
            &self,
            key: &str,
            _expires_in: StdDuration,
        ) -> Result<Option<SignedObjectUrl>> {
            Ok(Some(SignedObjectUrl {
                url: format!("https://objects.example/{key}"),
                expires_at_unix_seconds: 1234,
            }))
        }
    }

    fn handler_with_objects(objects: Arc<dyn ObjectStore + Send + Sync>) -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        )
        .objects(objects)
        .build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(None, control_plane)),
        }
    }

    fn handler_with_auth() -> GrpcGatewayHandler {
        let control_plane = ControlPlane::builder(
            Arc::new(MockKvStore::default()),
            Arc::new(RecordingPubSub::default()),
        )
        .build();
        GrpcGatewayHandler {
            gateway: Arc::new(Gateway::from_control_plane(
                Some(AuthConfig::jwt_platform()),
                control_plane,
            )),
        }
    }

    #[test]
    fn normalize_logical_path_rejects_root() {
        assert!(normalize_logical_path("/").is_err());
    }

    #[test]
    fn normalize_logical_path_trims_trailing_slash() {
        assert_eq!(
            normalize_logical_path("/memory/brand/").unwrap(),
            "/memory/brand"
        );
    }

    #[test]
    fn normalize_logical_path_rejects_backslashes() {
        assert!(normalize_logical_path("/memory\\brand.md").is_err());
    }

    #[test]
    fn normalize_logical_path_rejects_duplicate_separators() {
        assert!(normalize_logical_path("//memory/brand.md").is_err());
        assert!(normalize_logical_path("/memory//brand.md").is_err());
    }

    #[test]
    fn normalize_logical_path_rejects_dot_segments() {
        assert!(normalize_logical_path("/memory/./brand.md").is_err());
        assert!(normalize_logical_path("/memory/../brand.md").is_err());
    }

    #[test]
    fn page_cursor_helpers_validate_tokens() {
        assert_eq!(normalize_resource_name_cursor("").unwrap(), None);
        assert_eq!(
            normalize_resource_name_cursor("brand-guidelines-md-abc123").unwrap(),
            Some("brand-guidelines-md-abc123".to_string())
        );
        assert!(normalize_resource_name_cursor("bad/token").is_err());
    }

    #[test]
    fn media_type_validation_rejects_malformed_values() {
        assert_eq!(
            normalize_media_type("").unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_media_type("text/markdown; charset=utf-8").unwrap(),
            "text/markdown; charset=utf-8"
        );
        assert!(normalize_media_type("text").is_err());
        assert!(normalize_media_type("text/plain\r\nx: y").is_err());
    }

    #[test]
    fn path_prefix_matching_respects_path_boundaries() {
        assert!(path_matches_prefix("/memory/foo", ""));
        assert!(path_matches_prefix("/memory/foo", "/memory/foo"));
        assert!(path_matches_prefix("/memory/foo/bar", "/memory/foo"));
        assert!(!path_matches_prefix("/memory/foobar", "/memory/foo"));
    }

    #[tokio::test]
    async fn signed_url_read_skips_unary_size_cap_and_inline_bytes() {
        let handler = handler_with_objects(Arc::new(SignedUrlOnlyObjectStore));
        let (content, signed_url, expires_at) = handler
            .read_object_content_or_signed_url(
                "cas/Tenant%3Aacme/files/file-1/sha",
                MAX_UNARY_FILE_CONTENT_BYTES as u64 + 1,
            )
            .await
            .unwrap();

        assert!(content.is_empty());
        assert_eq!(
            signed_url,
            "https://objects.example/cas/Tenant%3Aacme/files/file-1/sha"
        );
        assert_eq!(expires_at, 1234);
    }

    #[test]
    fn handle_audience_requires_matching_caller_identity() {
        let grant = data_proto::HandleGrant {
            audience_agent: "writer".to_string(),
            audience_session_id: "session-1".to_string(),
            ..Default::default()
        };
        assert!(validate_handle_audience(
            &grant,
            HandleCaller {
                agent: "writer".to_string(),
                session_id: "session-1".to_string(),
            }
        )
        .is_ok());
        assert!(validate_handle_audience(
            &grant,
            HandleCaller {
                agent: "critic".to_string(),
                session_id: "session-1".to_string(),
            }
        )
        .is_err());
        assert!(validate_handle_audience(&grant, HandleCaller::default()).is_err());
    }

    #[test]
    fn handle_caller_uses_authenticated_claims_when_auth_is_enabled() {
        let handler = handler_with_auth();
        let mut req = Request::new(());
        req.metadata_mut()
            .insert(HANDLE_CALLER_AGENT_HEADER, "spoofed".parse().unwrap());
        req.metadata_mut().insert(
            HANDLE_CALLER_SESSION_HEADER,
            "spoofed-session".parse().unwrap(),
        );
        req.extensions_mut().insert(Claims {
            iss: None,
            sub: "subject".to_string(),
            aud: "talon-gateway".to_string(),
            iat: None,
            exp: usize::MAX,
            ns: Some("Tenant:test".to_string()),
            agent: Some("writer".to_string()),
            session: Some("session-1".to_string()),
            channel: None,
            origins: Vec::new(),
            grants: Vec::new(),
        });

        let caller = handle_caller_from_request(&handler, &req);

        assert_eq!(caller.agent, "writer");
        assert_eq!(caller.session_id, "session-1");
    }

    #[test]
    fn to_status_maps_common_client_errors() {
        assert_eq!(
            to_status(not_found("File 'missing' not found")).code(),
            tonic::Code::NotFound
        );
        assert_eq!(
            to_status(invalid_argument("path must be absolute")).code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            to_status(permission_denied(
                "handle audience agent does not match caller"
            ))
            .code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            to_status(Status::failed_precondition("missing objectRef").into()).code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(
            to_status(anyhow!("database failed")).code(),
            tonic::Code::Internal
        );
    }
}
