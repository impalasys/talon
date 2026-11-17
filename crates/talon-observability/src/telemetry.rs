// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use anyhow::Result;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    Resource,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const TALON_LOG_FORMAT_ENV: &str = "TALON_LOG_FORMAT";
const TALON_LOG_ANSI_ENV: &str = "TALON_LOG_ANSI";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Compact,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogConfig {
    format: LogFormat,
    ansi: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub service_name: String,
    pub sample_ratio: f64,
}

pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = &self.tracer_provider {
            if let Err(err) = provider.shutdown() {
                eprintln!("failed to shut down OpenTelemetry tracer provider: {err:?}");
            }
        }
    }
}

pub fn init_from_env(default_service_name: &str) -> Result<TelemetryGuard> {
    let config = TelemetryConfig::from_env(default_service_name);
    init(config)
}

pub fn init(config: TelemetryConfig) -> Result<TelemetryGuard> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let log_config = LogConfig::from_env();

    let Some(endpoint) = config.enabled.then_some(config.endpoint).flatten() else {
        match log_config.format {
            LogFormat::Compact => {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(tracing_subscriber::fmt::layer().with_ansi(log_config.ansi))
                    .try_init()
                    .ok();
            }
            LogFormat::Json => {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(
                        tracing_subscriber::fmt::layer()
                            .json()
                            .flatten_event(true)
                            .with_current_span(true)
                            .with_span_list(true)
                            .with_ansi(false),
                    )
                    .try_init()
                    .ok();
            }
        }
        return Ok(TelemetryGuard {
            tracer_provider: None,
        });
    };

    let service_name = config.service_name.clone();
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            config.sample_ratio,
        ))))
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(
            Resource::builder()
                .with_service_name(config.service_name)
                .build(),
        )
        .with_batch_exporter(exporter)
        .build();
    let tracer = tracer_provider.tracer(service_name);
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    match log_config.format {
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(tracing_subscriber::fmt::layer().with_ansi(log_config.ansi))
                .try_init()
                .ok();
        }
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .flatten_event(true)
                        .with_current_span(true)
                        .with_span_list(true)
                        .with_ansi(false),
                )
                .try_init()
                .ok();
        }
    }

    Ok(TelemetryGuard {
        tracer_provider: Some(tracer_provider),
    })
}

impl LogConfig {
    fn from_env() -> Self {
        Self::from_getter(|name| std::env::var(name).ok())
    }

    fn from_getter<F>(mut get: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        let format = match get(TALON_LOG_FORMAT_ENV)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "json" => LogFormat::Json,
            _ => LogFormat::Compact,
        };
        let ansi = get(TALON_LOG_ANSI_ENV)
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(format != LogFormat::Json);

        Self { format, ansi }
    }
}

impl TelemetryConfig {
    pub fn from_env(default_service_name: &str) -> Self {
        Self::from_getter(default_service_name, |name| std::env::var(name).ok())
    }

    fn from_getter<F>(default_service_name: &str, mut get: F) -> Self
    where
        F: FnMut(&str) -> Option<String>,
    {
        let enabled = get("TALON_OTEL_ENABLED")
            .map(|value| {
                matches!(
                    value.as_str(),
                    "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
                )
            })
            .unwrap_or(false);
        let endpoint = get("OTEL_EXPORTER_OTLP_ENDPOINT").filter(|value| !value.trim().is_empty());
        let service_name = get("OTEL_SERVICE_NAME")
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_service_name.to_string());
        let sample_ratio = get("TALON_OTEL_SAMPLE_RATIO")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .unwrap_or(1.0);

        Self {
            enabled,
            endpoint,
            service_name,
            sample_ratio,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LogConfig, LogFormat, TelemetryConfig};

    fn config_with(vars: &[(&str, &str)]) -> TelemetryConfig {
        TelemetryConfig::from_getter("talon-worker", |name| {
            vars.iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
        })
    }

    #[test]
    fn telemetry_config_is_disabled_by_default() {
        assert_eq!(
            config_with(&[]),
            TelemetryConfig {
                enabled: false,
                endpoint: None,
                service_name: "talon-worker".to_string(),
                sample_ratio: 1.0,
            }
        );
    }

    #[test]
    fn telemetry_config_reads_enabled_endpoint_service_and_sampling() {
        assert_eq!(
            config_with(&[
                ("TALON_OTEL_ENABLED", "true"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://jaeger:4317"),
                ("OTEL_SERVICE_NAME", "talon-bench-worker"),
                ("TALON_OTEL_SAMPLE_RATIO", "0.25"),
            ]),
            TelemetryConfig {
                enabled: true,
                endpoint: Some("http://jaeger:4317".to_string()),
                service_name: "talon-bench-worker".to_string(),
                sample_ratio: 0.25,
            }
        );
    }

    #[test]
    fn telemetry_config_rejects_invalid_sample_ratio() {
        assert_eq!(
            config_with(&[
                ("TALON_OTEL_ENABLED", "1"),
                ("TALON_OTEL_SAMPLE_RATIO", "2.0"),
            ])
            .sample_ratio,
            1.0
        );
    }

    #[test]
    fn log_config_defaults_to_compact_with_ansi() {
        assert_eq!(
            LogConfig::from_getter(|_| None),
            LogConfig {
                format: LogFormat::Compact,
                ansi: true,
            }
        );
    }

    #[test]
    fn log_config_json_disables_ansi_by_default() {
        assert_eq!(
            LogConfig::from_getter(|name| {
                (name == "TALON_LOG_FORMAT").then(|| "json".to_string())
            }),
            LogConfig {
                format: LogFormat::Json,
                ansi: false,
            }
        );
    }

    #[test]
    fn log_config_respects_explicit_ansi_override() {
        assert_eq!(
            LogConfig::from_getter(|name| match name {
                "TALON_LOG_FORMAT" => Some("compact".to_string()),
                "TALON_LOG_ANSI" => Some("0".to_string()),
                _ => None,
            }),
            LogConfig {
                format: LogFormat::Compact,
                ansi: false,
            }
        );
    }
}
