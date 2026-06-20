// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use prost::Message;

pub(crate) fn transcode_proto<From, To>(value: &From) -> Result<To>
where
    From: Message,
    To: Message + Default,
{
    let mut bytes = Vec::new();
    value
        .encode(&mut bytes)
        .context("Failed to encode protobuf message")?;
    To::decode(bytes.as_slice()).context("Failed to decode protobuf message")
}

pub(crate) fn to_sdk_resource_manifest(
    manifest: &crate::gateway::rpc::resources_proto::ResourceManifest,
) -> Result<talon_client::resources::ResourceManifest> {
    transcode_proto(manifest)
}

pub(crate) fn to_internal_resource(
    resource: &talon_client::resources::Resource,
) -> Result<crate::gateway::rpc::resources_proto::Resource> {
    transcode_proto(resource)
}
