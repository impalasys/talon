#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowManifest {
    api_version: String,
    kind: String,
    metadata: ObjectMetaManifest,
    spec: WorkflowSpecManifest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowSpecManifest {
    description: String,
    input_schema: serde_yaml::Value,
    output_schema: serde_yaml::Value,
    steps: Vec<WorkflowStepManifest>,
    output: serde_yaml::Value,
    concurrency: u32,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepManifest {
    id: String,
    #[serde(rename = "type")]
    type_name: String,
    after: Vec<String>,
    when: serde_yaml::Value,
    agent: String,
    prompt: String,
    tool: String,
    input: serde_yaml::Value,
    workflow: String,
    output: Option<WorkflowStepOutputPolicyManifest>,
    resume_schema: serde_yaml::Value,
    retry: Option<WorkflowStepRetryPolicyManifest>,
    timeout: String,
    duration: String,
    until: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepOutputPolicyManifest {
    format: String,
    schema: serde_yaml::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct WorkflowStepRetryPolicyManifest {
    #[serde(default = "default_workflow_retry_max_attempts")]
    max_attempts: u32,
    #[serde(default = "default_workflow_retry_initial_backoff_seconds")]
    initial_backoff_seconds: i64,
    #[serde(default = "default_workflow_retry_max_backoff_seconds")]
    max_backoff_seconds: i64,
    #[serde(default = "default_workflow_retry_multiplier")]
    multiplier: f64,
}

fn default_workflow_retry_max_attempts() -> u32 {
    3
}

fn default_workflow_retry_initial_backoff_seconds() -> i64 {
    1
}

fn default_workflow_retry_max_backoff_seconds() -> i64 {
    30
}

fn default_workflow_retry_multiplier() -> f64 {
    2.0
}

impl Default for WorkflowStepRetryPolicyManifest {
    fn default() -> Self {
        Self {
            max_attempts: default_workflow_retry_max_attempts(),
            initial_backoff_seconds: default_workflow_retry_initial_backoff_seconds(),
            max_backoff_seconds: default_workflow_retry_max_backoff_seconds(),
            multiplier: default_workflow_retry_multiplier(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public parse API
// ---------------------------------------------------------------------------
