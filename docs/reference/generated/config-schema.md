---
title: Config Schema
---

This page summarizes the major configuration messages exposed by Talon's runtime configuration proto.

## `TalonConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `providers` | `map<string, LlmProviderConfig>` | - |
| `database` | `DatabaseConfig` | - |
| `server` | `ServerConfig` | - |
| `default_provider` | `string` | - |
| `workspace_dir` | `string` | - |
| `control_plane` | `ControlPlaneConfig` | - |
| `controllers` | `map<string, ControllerConfig>` | - |
| `trust` | `TrustConfig` | - |
| `platform_auth` | `PlatformAuthConfig` | - |

## `PlatformAuthConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `jwt_issuer` | `JwtIssuerConfig` | - |

## `JwtIssuerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `issuer` | `string` | Stable HTTPS issuer for Talon-issued JWTs and well-known metadata. |

## `TrustConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `oidc` | `OidcTrustEntry` | repeated |

## `OidcTrustEntry`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `issuer` | `string` | - |
| `audiences` | `string` | repeated |
| `allowed_domains` | `string` | repeated |
| `allowed_emails` | `string` | repeated |
| `jwks_url` | `string` | - |
| `clock_skew_seconds` | `uint32` | - |
| `grants` | `OidcTrustGrant` | repeated |

## `OidcTrustGrant`

| Field | Type | Notes |
| --- | --- | --- |
| `kind` | `Kind` | - |
| `namespace` | `string` | - |
| `agent` | `string` | - |
| `session` | `string` | - |
| `channel` | `string` | - |

## `ControllerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `enabled` | `bool` | - |
| `workers` | `uint32` | - |

## `LlmProviderConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `openai` | `OpenAiConfig` | oneof (config) |
| `anthropic` | `AnthropicConfig` | oneof (config) |
| `google` | `GoogleConfig` | oneof (config) |
| `openai_compatible` | `GenericConfig` | oneof (config) |

## `OpenAiConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `model` | `string` | - |
| `api_key` | `Secret` | - |
| `org_id` | `string` | - |

## `AnthropicConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `model` | `string` | - |
| `api_key` | `Secret` | - |

## `GoogleConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `model` | `string` | - |
| `api_key` | `Secret` | - |

## `GenericConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `name` | `string` | - |
| `base_url` | `string` | - |
| `model` | `string` | - |
| `api_key` | `Secret` | - |

## `Secret`

| Field | Type | Notes |
| --- | --- | --- |
| `plain` | `string` | oneof (source) |
| `ref` | `SecretRef` | oneof (source) |

## `SecretRef`

| Field | Type | Notes |
| --- | --- | --- |
| `source` | `Source` | - |
| `key` | `string` | - |

## `DatabaseConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `data_dir` | `string` | - |
| `driver` | `string` | - |
| `url` | `Secret` | - |

## `MessageBrokerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `driver` | `string` | - |

## `LocalObjectStoreConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `path` | `string` | - |

## `GcsObjectStoreConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `bucket` | `string` | - |
| `prefix` | `string` | - |
| `api_base_url` | `string` | - |

## `S3ObjectStoreConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `bucket` | `string` | - |
| `prefix` | `string` | - |
| `region` | `string` | - |
| `endpoint_url` | `string` | - |
| `force_path_style` | `bool` | - |

## `ObjectStoreConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `local` | `LocalObjectStoreConfig` | oneof (backend) |
| `gcs` | `GcsObjectStoreConfig` | oneof (backend) |
| `s3` | `S3ObjectStoreConfig` | oneof (backend) |

## `SchedulerCallbackAuthConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `shared_secret` | `Secret` | oneof (auth) |
| `google_oidc` | `GoogleOidcAuthConfig` | oneof (auth) |

## `GoogleOidcAuthConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `audience` | `string` | - |
| `service_account_email` | `string` | - |

## `CloudTasksSchedulerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `project_id` | `string` | - |
| `location` | `string` | - |
| `queue` | `string` | - |
| `target_url` | `string` | - |
| `callback_auth` | `SchedulerCallbackAuthConfig` | - |

## `SchedulerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `cloud_tasks` | `CloudTasksSchedulerConfig` | oneof (backend) |

## `ControlPlaneConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `database` | `DatabaseConfig` | - |
| `message_broker` | `MessageBrokerConfig` | - |
| `scheduler` | `SchedulerConfig` | - |
| `object_store` | `ObjectStoreConfig` | - |
| `documents` | `DatabaseConfig` | - |

## `ServerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `host` | `string` | - |
| `port` | `uint32` | - |
