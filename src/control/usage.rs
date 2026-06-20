// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use crate::control::{keys, KeyValueStore};
use crate::gateway::rpc::{harness_proto, resources_proto};
use anyhow::{anyhow, bail, Context, Result};
use prost::Message;
use std::time::Duration;

const MAX_CAS_RETRIES: usize = 16;

pub const METRIC_LLM_REQUESTS: &str = "llm.requests";
pub const METRIC_LLM_INPUT_TOKENS: &str = "llm.inputTokens";
pub const METRIC_LLM_OUTPUT_TOKENS: &str = "llm.outputTokens";
pub const METRIC_LLM_REASONING_TOKENS: &str = "llm.reasoningTokens";
pub const METRIC_LLM_TOTAL_TOKENS: &str = "llm.totalTokens";
pub const METRIC_AGENT_SESSIONS: &str = "agent.sessions";
pub const METRIC_TOOL_CALLS: &str = "tool.calls";

#[derive(Debug, Clone)]
pub struct UsageSubject {
    pub namespace: String,
    pub agent: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct UsageCharge {
    pub metric: &'static str,
    pub delta: u64,
}

#[derive(Debug, Clone)]
struct MatchedLimit {
    policy_namespace: String,
    policy_name: String,
    rule_id: String,
    limit: resources_proto::UsageLimit,
    window: Window,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    seconds: u64,
}

pub fn llm_usage_charges(usage: Option<&harness_proto::ChatUsage>) -> Vec<UsageCharge> {
    let mut charges = vec![UsageCharge {
        metric: METRIC_LLM_REQUESTS,
        delta: 1,
    }];
    if let Some(usage) = usage {
        push_nonzero(&mut charges, METRIC_LLM_INPUT_TOKENS, usage.input_tokens);
        push_nonzero(&mut charges, METRIC_LLM_OUTPUT_TOKENS, usage.output_tokens);
        push_nonzero(
            &mut charges,
            METRIC_LLM_REASONING_TOKENS,
            usage.reasoning_tokens,
        );
        push_nonzero(&mut charges, METRIC_LLM_TOTAL_TOKENS, usage.total_tokens);
    }
    charges
}

pub async fn check_namespace_usage(
    kv: &(dyn KeyValueStore + Send + Sync),
    subject: &UsageSubject,
    metrics: &[&str],
    now_seconds: i64,
) -> Result<()> {
    for limit in matched_limits(kv, subject, metrics, now_seconds).await? {
        let key = counter_key(
            &limit.policy_namespace,
            &limit.policy_name,
            &limit.rule_id,
            window_start(now_seconds, limit.window),
        );
        let used = read_counter(kv, &key).await?;
        if used >= limit.limit.max {
            bail!(
                "usage quota exceeded for metric '{}' in namespace '{}': used {} of {}",
                limit.limit.metric,
                subject.namespace,
                used,
                limit.limit.max
            );
        }
    }
    Ok(())
}

pub async fn charge_namespace_usage(
    kv: &(dyn KeyValueStore + Send + Sync),
    subject: &UsageSubject,
    charges: &[UsageCharge],
    now_seconds: i64,
) -> Result<()> {
    let metrics = charges
        .iter()
        .map(|charge| charge.metric)
        .collect::<Vec<_>>();
    for limit in matched_limits(kv, subject, &metrics, now_seconds).await? {
        let Some(charge) = charges
            .iter()
            .find(|charge| charge.metric == limit.limit.metric)
        else {
            continue;
        };
        if charge.delta == 0 {
            continue;
        }
        let key = counter_key(
            &limit.policy_namespace,
            &limit.policy_name,
            &limit.rule_id,
            window_start(now_seconds, limit.window),
        );
        increment_counter_cas(kv, &key, charge.delta).await?;
    }
    Ok(())
}

pub async fn populate_usage_policy_status(
    kv: &(dyn KeyValueStore + Send + Sync),
    resource: &mut resources_proto::Resource,
    now_seconds: i64,
) -> Result<()> {
    if resource.kind != "UsagePolicy" {
        return Ok(());
    }
    let Some(meta) = resource.metadata.as_ref() else {
        return Ok(());
    };
    let Some(resources_proto::resource_spec::Kind::UsagePolicy(spec)) =
        resource.spec.as_ref().and_then(|spec| spec.kind.as_ref())
    else {
        return Ok(());
    };
    validate_usage_policy_spec(spec)?;
    let mut hard = Vec::with_capacity(spec.hard.len());
    for limit in &spec.hard {
        let window = parse_window(&limit.window)?;
        let start = window_start(now_seconds, window);
        let reset_at = start.saturating_add(i64::try_from(window.seconds).unwrap_or(i64::MAX));
        let rule_id = rule_id(limit);
        let key = counter_key(&meta.namespace, &meta.name, &rule_id, start);
        let used = read_counter(kv, &key).await?;
        hard.push(resources_proto::UsageLimitStatus {
            selector: limit.selector.clone(),
            metric: limit.metric.clone(),
            max: limit.max,
            window: limit.window.clone(),
            window_start: start,
            reset_at,
            used,
            remaining: limit.max.saturating_sub(used),
            exceeded: used >= limit.max,
        });
    }
    let observed_generation = meta.generation;
    resource.status = Some(resources_proto::ResourceStatus {
        kind: Some(resources_proto::resource_status::Kind::UsagePolicy(
            resources_proto::UsagePolicyStatus {
                observed_generation,
                phase: "Active".to_string(),
                conditions: Vec::new(),
                hard,
            },
        )),
    });
    Ok(())
}

pub fn validate_usage_policy_spec(spec: &resources_proto::UsagePolicySpec) -> Result<()> {
    match namespace_scope(spec.namespace_scope.as_str())? {
        NamespaceScope::Recursive | NamespaceScope::SelfOnly => {}
    }
    if spec.hard.is_empty() {
        bail!("UsagePolicy spec.hard must contain at least one limit");
    }
    for limit in &spec.hard {
        validate_metric(&limit.metric)?;
        if limit.max == 0 {
            bail!(
                "UsagePolicy limit '{}' max must be greater than zero",
                limit.metric
            );
        }
        parse_window(&limit.window)?;
        if let Some(selector) = limit.selector.as_ref() {
            let is_llm = is_llm_metric(&limit.metric);
            if !is_llm && (!selector.provider.is_empty() || !selector.model.is_empty()) {
                bail!(
                    "UsagePolicy limit '{}' cannot use provider/model selectors",
                    limit.metric
                );
            }
        }
    }
    Ok(())
}

pub async fn increment_counter_cas(
    kv: &(dyn KeyValueStore + Send + Sync),
    key: &keys::ResourceKey,
    delta: u64,
) -> Result<u64> {
    let mut backoff = Duration::from_millis(1);
    for _ in 0..MAX_CAS_RETRIES {
        let current_bytes = kv.get(key).await?;
        let current = match current_bytes.as_deref() {
            Some(bytes) => decode_counter(bytes)
                .with_context(|| format!("failed to decode usage counter {}", key))?,
            None => 0,
        };
        let next = current
            .checked_add(delta)
            .ok_or_else(|| anyhow!("usage counter {} overflowed", key))?;
        let next_bytes = encode_counter(next);
        if kv
            .compare_and_swap(key, current_bytes.as_deref(), &next_bytes)
            .await?
        {
            return Ok(next);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_millis(25));
    }
    bail!(
        "failed to increment usage counter {} after CAS retries",
        key
    )
}

pub async fn read_counter(
    kv: &(dyn KeyValueStore + Send + Sync),
    key: &keys::ResourceKey,
) -> Result<u64> {
    match kv.get(key).await? {
        Some(bytes) => decode_counter(&bytes),
        None => Ok(0),
    }
}

fn push_nonzero(charges: &mut Vec<UsageCharge>, metric: &'static str, delta: u64) {
    if delta > 0 {
        charges.push(UsageCharge { metric, delta });
    }
}

async fn matched_limits(
    kv: &(dyn KeyValueStore + Send + Sync),
    subject: &UsageSubject,
    metrics: &[&str],
    now_seconds: i64,
) -> Result<Vec<MatchedLimit>> {
    let mut matches = Vec::new();
    for namespace in ancestor_namespaces(&subject.namespace) {
        let entries = kv
            .list_entries(&keys::ResourceParent::root(&namespace).list(Some("UsagePolicy")))
            .await?;
        for (key, value) in entries {
            let policy =
                resources_proto::UsagePolicy::decode(value.as_slice()).with_context(|| {
                    format!(
                        "failed to decode UsagePolicy {}/{} while evaluating usage",
                        key.namespace, key.name
                    )
                })?;
            let Some(meta) = policy.metadata.as_ref() else {
                continue;
            };
            let Some(spec) = policy.spec.as_ref() else {
                continue;
            };
            validate_usage_policy_spec(spec)?;
            if !policy_applies(
                &meta.namespace,
                spec.namespace_scope.as_str(),
                &subject.namespace,
            )? {
                continue;
            }
            for limit in &spec.hard {
                if !metrics.iter().any(|metric| *metric == limit.metric) {
                    continue;
                }
                if !selector_matches(limit.selector.as_ref(), subject) {
                    continue;
                }
                let window = parse_window(&limit.window)?;
                let _ = window_start(now_seconds, window);
                matches.push(MatchedLimit {
                    policy_namespace: meta.namespace.clone(),
                    policy_name: meta.name.clone(),
                    rule_id: rule_id(limit),
                    limit: limit.clone(),
                    window,
                });
            }
        }
    }
    Ok(matches)
}

fn ancestor_namespaces(namespace: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = namespace.trim();
    while !current.is_empty() {
        result.push(current.to_string());
        let Some((parent, _)) = current.rsplit_once(':') else {
            break;
        };
        current = parent;
    }
    result
}

fn policy_applies(policy_namespace: &str, scope: &str, target_namespace: &str) -> Result<bool> {
    match namespace_scope(scope)? {
        NamespaceScope::SelfOnly => Ok(policy_namespace == target_namespace),
        NamespaceScope::Recursive => Ok(policy_namespace == target_namespace
            || target_namespace
                .strip_prefix(policy_namespace)
                .is_some_and(|rest| rest.starts_with(':'))),
    }
}

fn selector_matches(
    selector: Option<&resources_proto::UsageSelector>,
    subject: &UsageSubject,
) -> bool {
    let Some(selector) = selector else {
        return true;
    };
    (selector.agent.is_empty() || selector.agent == subject.agent)
        && (selector.provider.is_empty() || selector.provider == subject.provider)
        && (selector.model.is_empty() || selector.model == subject.model)
}

#[derive(Debug, Clone, Copy)]
enum NamespaceScope {
    Recursive,
    SelfOnly,
}

fn namespace_scope(scope: &str) -> Result<NamespaceScope> {
    match scope.trim() {
        "" | "recursive" => Ok(NamespaceScope::Recursive),
        "self" => Ok(NamespaceScope::SelfOnly),
        value => bail!("UsagePolicy namespaceScope must be 'recursive' or 'self', got '{value}'"),
    }
}

fn validate_metric(metric: &str) -> Result<()> {
    match metric {
        METRIC_LLM_REQUESTS
        | METRIC_LLM_INPUT_TOKENS
        | METRIC_LLM_OUTPUT_TOKENS
        | METRIC_LLM_REASONING_TOKENS
        | METRIC_LLM_TOTAL_TOKENS
        | METRIC_AGENT_SESSIONS
        | METRIC_TOOL_CALLS => Ok(()),
        _ => bail!("unsupported UsagePolicy metric '{metric}'"),
    }
}

fn is_llm_metric(metric: &str) -> bool {
    metric.starts_with("llm.")
}

fn parse_window(value: &str) -> Result<Window> {
    let value = value.trim();
    if value.len() < 2 {
        bail!("UsagePolicy window must be a duration like 1m, 5h, or 7d");
    }
    let (number, unit) = value.split_at(value.len() - 1);
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("UsagePolicy window must use an integer duration like 1m, 5h, or 7d");
    }
    let amount: u64 = number.parse()?;
    if amount == 0 {
        bail!("UsagePolicy window must be greater than zero");
    }
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        "w" => 7 * 24 * 60 * 60,
        _ => bail!("UsagePolicy window unit must be one of s, m, h, d, or w"),
    };
    Ok(Window {
        seconds: amount
            .checked_mul(multiplier)
            .ok_or_else(|| anyhow!("UsagePolicy window is too large"))?,
    })
}

fn window_start(now_seconds: i64, window: Window) -> i64 {
    let window_seconds = i64::try_from(window.seconds).unwrap_or(i64::MAX);
    now_seconds.div_euclid(window_seconds) * window_seconds
}

fn counter_key(
    policy_namespace: &str,
    policy_name: &str,
    rule_id: &str,
    window_start: i64,
) -> keys::ResourceKey {
    keys::ResourceKey::new(
        policy_namespace,
        &[("UsagePolicy", policy_name)],
        "UsageCounter",
        &format!("{rule_id}:{window_start}"),
    )
}

fn rule_id(limit: &resources_proto::UsageLimit) -> String {
    let selector = limit.selector.as_ref();
    let canonical = format!(
        "metric={};window={};agent={};provider={};model={}",
        limit.metric,
        limit.window,
        selector.map(|s| s.agent.as_str()).unwrap_or_default(),
        selector.map(|s| s.provider.as_str()).unwrap_or_default(),
        selector.map(|s| s.model.as_str()).unwrap_or_default(),
    );
    format!("{:016x}", fnv1a64(canonical.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn encode_counter(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_counter(bytes: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| anyhow!("usage counter value must be exactly 8 bytes"))?;
    Ok(u64::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ProtoKeyValueStoreExt;
    use crate::test_support::MockKvStore;
    use prost::Message;

    fn limit(metric: &str, window: &str) -> resources_proto::UsageLimit {
        resources_proto::UsageLimit {
            selector: None,
            metric: metric.to_string(),
            max: 10,
            window: window.to_string(),
        }
    }

    #[test]
    fn parses_fixed_windows() {
        assert_eq!(parse_window("1s").unwrap().seconds, 1);
        assert_eq!(parse_window("1m").unwrap().seconds, 60);
        assert_eq!(parse_window("5h").unwrap().seconds, 18_000);
        assert_eq!(parse_window("7d").unwrap().seconds, 604_800);
        assert_eq!(parse_window("4w").unwrap().seconds, 2_419_200);

        for value in ["", "0s", "1.5h", "1h30m", "5", "5mo"] {
            assert!(parse_window(value).is_err(), "{value} should be invalid");
        }
    }

    #[test]
    fn computes_epoch_aligned_window_start() {
        assert_eq!(window_start(61, parse_window("1m").unwrap()), 60);
        assert_eq!(window_start(18_001, parse_window("5h").unwrap()), 18_000);
    }

    #[test]
    fn validates_metric_selector_pairs() {
        validate_usage_policy_spec(&resources_proto::UsagePolicySpec {
            namespace_scope: String::new(),
            hard: vec![resources_proto::UsageLimit {
                selector: Some(resources_proto::UsageSelector {
                    agent: String::new(),
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                }),
                metric: METRIC_LLM_REQUESTS.to_string(),
                max: 1,
                window: "1m".to_string(),
            }],
        })
        .unwrap();

        assert!(
            validate_usage_policy_spec(&resources_proto::UsagePolicySpec {
                namespace_scope: "self".to_string(),
                hard: vec![resources_proto::UsageLimit {
                    selector: Some(resources_proto::UsageSelector {
                        agent: String::new(),
                        provider: "openai".to_string(),
                        model: String::new(),
                    }),
                    metric: METRIC_TOOL_CALLS.to_string(),
                    max: 1,
                    window: "1m".to_string(),
                }],
            })
            .is_err()
        );
    }

    #[test]
    fn matches_recursive_and_self_scope() {
        assert!(policy_applies("acme", "", "acme").unwrap());
        assert!(policy_applies("acme", "recursive", "acme:prod").unwrap());
        assert!(!policy_applies("acme", "self", "acme:prod").unwrap());
        assert!(!policy_applies("acme", "recursive", "acme-prod").unwrap());
    }

    #[tokio::test]
    async fn increments_counter_with_cas_encoding() {
        let kv = MockKvStore::new();
        let key = counter_key("acme", "policy", "rule", 120);
        assert_eq!(increment_counter_cas(&kv, &key, 2).await.unwrap(), 2);
        assert_eq!(increment_counter_cas(&kv, &key, 3).await.unwrap(), 5);
        assert_eq!(read_counter(&kv, &key).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn rejects_malformed_counter_values() {
        let kv = MockKvStore::new();
        let key = counter_key("acme", "policy", "rule", 120);
        kv.set(&key, b"bad").await.unwrap();
        assert!(read_counter(&kv, &key).await.is_err());
        assert!(increment_counter_cas(&kv, &key, 1).await.is_err());
    }

    #[tokio::test]
    async fn charges_matching_policy_limit() {
        let kv = MockKvStore::new();
        let policy = resources_proto::UsagePolicy {
            metadata: Some(resources_proto::ResourceMeta {
                name: "policy".to_string(),
                namespace: "acme".to_string(),
                ..Default::default()
            }),
            spec: Some(resources_proto::UsagePolicySpec {
                namespace_scope: "recursive".to_string(),
                hard: vec![limit(METRIC_LLM_TOTAL_TOKENS, "1m")],
            }),
            status: None,
        };
        kv.set_msg(
            &keys::ResourceKey::new("acme", &[], "UsagePolicy", "policy"),
            &policy,
        )
        .await
        .unwrap();
        let subject = UsageSubject {
            namespace: "acme:prod".to_string(),
            agent: "agent".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
        };
        charge_namespace_usage(
            &kv,
            &subject,
            &[UsageCharge {
                metric: METRIC_LLM_TOTAL_TOKENS,
                delta: 12,
            }],
            65,
        )
        .await
        .unwrap();
        let mut resource = resources_proto::Resource {
            kind: "UsagePolicy".to_string(),
            metadata: policy.metadata.clone(),
            spec: Some(resources_proto::ResourceSpec {
                kind: Some(resources_proto::resource_spec::Kind::UsagePolicy(
                    policy.spec.clone().unwrap(),
                )),
            }),
            status: None,
            ..Default::default()
        };
        populate_usage_policy_status(&kv, &mut resource, 65)
            .await
            .unwrap();
        let status = match resource.status.unwrap().kind.unwrap() {
            resources_proto::resource_status::Kind::UsagePolicy(status) => status,
            _ => unreachable!(),
        };
        assert_eq!(status.hard[0].used, 12);
        assert!(status.hard[0].exceeded);
    }

    #[test]
    fn usage_policy_proto_roundtrips() {
        let policy = resources_proto::UsagePolicy {
            metadata: Some(resources_proto::ResourceMeta::default()),
            spec: Some(resources_proto::UsagePolicySpec {
                namespace_scope: "recursive".to_string(),
                hard: vec![limit(METRIC_LLM_REQUESTS, "5h")],
            }),
            status: None,
        };
        let decoded =
            resources_proto::UsagePolicy::decode(policy.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded.spec.unwrap().hard[0].window, "5h");
    }
}
