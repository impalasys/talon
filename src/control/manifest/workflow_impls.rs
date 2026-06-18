impl WorkflowSpecManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowSpec> {
        Ok(resources_proto::WorkflowSpec {
            description: self.description,
            input_schema_json: yaml_value_to_json_string(self.input_schema)?,
            output_schema_json: yaml_value_to_json_string(self.output_schema)?,
            steps: self
                .steps
                .into_iter()
                .map(WorkflowStepManifest::into_proto)
                .collect::<Result<Vec<_>>>()?,
            output_json: yaml_value_to_json_string(self.output)?,
            concurrency: self.concurrency,
        })
    }

    fn from_proto(spec: &resources_proto::WorkflowSpec) -> Result<Self> {
        Ok(Self {
            description: spec.description.clone(),
            input_schema: json_string_to_yaml_value(&spec.input_schema_json)?,
            output_schema: json_string_to_yaml_value(&spec.output_schema_json)?,
            steps: spec
                .steps
                .iter()
                .map(WorkflowStepManifest::from_proto)
                .collect::<Result<Vec<_>>>()?,
            output: json_string_to_yaml_value(&spec.output_json)?,
            concurrency: spec.concurrency,
        })
    }
}

impl WorkflowStepManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStep> {
        Ok(resources_proto::WorkflowStep {
            id: self.id,
            r#type: self.type_name,
            after: self.after,
            when_json: yaml_value_to_json_string(self.when)?,
            agent: self.agent,
            prompt: self.prompt,
            tool: self.tool,
            input_json: yaml_value_to_json_string(self.input)?,
            workflow: self.workflow,
            output: self
                .output
                .map(WorkflowStepOutputPolicyManifest::into_proto)
                .transpose()?,
            resume_schema_json: yaml_value_to_json_string(self.resume_schema)?,
            retry: self
                .retry
                .map(WorkflowStepRetryPolicyManifest::into_proto)
                .transpose()?,
            timeout: self.timeout,
            wait_duration: self.duration,
            wait_until: self.until,
        })
    }

    fn from_proto(step: &resources_proto::WorkflowStep) -> Result<Self> {
        Ok(Self {
            id: step.id.clone(),
            type_name: step.r#type.clone(),
            after: step.after.clone(),
            when: json_string_to_yaml_value(&step.when_json)?,
            agent: step.agent.clone(),
            prompt: step.prompt.clone(),
            tool: step.tool.clone(),
            input: json_string_to_yaml_value(&step.input_json)?,
            workflow: step.workflow.clone(),
            output: step
                .output
                .as_ref()
                .map(WorkflowStepOutputPolicyManifest::from_proto)
                .transpose()?,
            resume_schema: json_string_to_yaml_value(&step.resume_schema_json)?,
            retry: step
                .retry
                .as_ref()
                .map(WorkflowStepRetryPolicyManifest::from_proto),
            timeout: step.timeout.clone(),
            duration: step.wait_duration.clone(),
            until: step.wait_until.clone(),
        })
    }
}

impl WorkflowStepOutputPolicyManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStepOutputPolicy> {
        Ok(resources_proto::WorkflowStepOutputPolicy {
            format: self.format,
            schema_json: yaml_value_to_json_string(self.schema)?,
        })
    }

    fn from_proto(policy: &resources_proto::WorkflowStepOutputPolicy) -> Result<Self> {
        Ok(Self {
            format: policy.format.clone(),
            schema: json_string_to_yaml_value(&policy.schema_json)?,
        })
    }
}

impl WorkflowStepRetryPolicyManifest {
    fn into_proto(self) -> Result<resources_proto::WorkflowStepRetryPolicy> {
        Ok(resources_proto::WorkflowStepRetryPolicy {
            max_attempts: self.max_attempts,
            initial_backoff_seconds: self.initial_backoff_seconds,
            max_backoff_seconds: self.max_backoff_seconds,
            multiplier: self.multiplier,
        })
    }

    fn from_proto(policy: &resources_proto::WorkflowStepRetryPolicy) -> Self {
        Self {
            max_attempts: policy.max_attempts,
            initial_backoff_seconds: policy.initial_backoff_seconds,
            max_backoff_seconds: policy.max_backoff_seconds,
            multiplier: policy.multiplier,
        }
    }
}
