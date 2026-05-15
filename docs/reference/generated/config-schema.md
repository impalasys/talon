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

## `ServerConfig`

| Field | Type | Notes |
| --- | --- | --- |
| `host` | `string` | - |
| `port` | `uint32` | - |
