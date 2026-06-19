pub(super) fn parse_raw_manifest(content: &str) -> Result<crate::control::manifest::RawManifest> {
    serde_yaml::from_str(content).context("Failed to parse manifest YAML")
}

pub(super) fn render_manifest_file(file: &str, vars: &[String]) -> Result<String> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("Failed to read manifest file: {}", file))?;
    let vars = parse_vars(vars)?;
    let rendered = render_manifest_template(&content, &vars)
        .with_context(|| format!("Failed to render manifest file: {}", file))?;
    resolve_manifest_sources(file, &rendered)
}

fn resolve_manifest_sources(file: &str, rendered: &str) -> Result<String> {
    let raw = parse_raw_manifest(rendered)?;
    if raw.kind != "Knowledge" {
        return Ok(rendered.to_string());
    }

    let mut manifest: serde_yaml::Value =
        serde_yaml::from_str(rendered).context("Failed to parse rendered Knowledge manifest")?;
    let file_manifest: KnowledgeManifestFile = serde_yaml::from_str(rendered)
        .context("Failed to parse Knowledge manifest source directives")?;

    let content = match (
        file_manifest.spec.content.clone(),
        file_manifest.spec.content_from_file.clone(),
    ) {
        (Some(content), None) => content,
        (None, Some(path)) => {
            let base_dir = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
            let full_path = canonicalize_manifest_path(base_dir, &path);
            fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "Failed to read Knowledge contentFromFile '{}'",
                    full_path.display()
                )
            })?
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("Knowledge manifest spec can set only one of content or contentFromFile")
        }
        (None, None) => {
            anyhow::bail!("Knowledge manifest spec must set one of content or contentFromFile")
        }
    };

    if let Some(spec) = manifest
        .get_mut("spec")
        .and_then(|value| value.as_mapping_mut())
    {
        spec.remove(&serde_yaml::Value::String("contentFromFile".to_string()));
        spec.insert(
            serde_yaml::Value::String("content".to_string()),
            serde_yaml::Value::String(content),
        );
    }

    serde_yaml::to_string(&manifest).context("Failed to serialize resolved Knowledge manifest")
}

fn canonicalize_manifest_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn parse_vars(entries: &[String]) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .with_context(|| format!("Invalid --var '{}', expected KEY=VALUE", entry))?;
        if key.is_empty() {
            anyhow::bail!("Invalid --var '{}', key cannot be empty", entry);
        }
        vars.insert(key.to_string(), value.to_string());
    }
    Ok(vars)
}

fn render_manifest_template(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template("manifest", template)
        .context("Failed to compile manifest template")?;
    let rendered = env
        .get_template("manifest")
        .context("Missing manifest template")?
        .render(context! { vars => vars })
        .context("Failed to render manifest template")?;
    Ok(rendered)
}
