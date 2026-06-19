pub(super) async fn sync_knowledge_dir(
    cli: &Cli,
    namespace: &str,
    dir: &str,
) -> Result<(usize, Vec<String>)> {
    let root = Path::new(dir);
    let files = collect_markdown_files(root)?;
    let existing: Vec<Knowledge> = knowledge_list(cli, namespace).await?;
    let existing_paths = existing
        .into_iter()
        .filter_map(|knowledge| knowledge.spec.map(|spec| spec.path))
        .collect::<std::collections::HashSet<_>>();
    let mut synced_paths = Vec::new();

    for file in files {
        let knowledge_path = relative_knowledge_path(root, &file)?;
        let content = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read knowledge file '{}'", file.display()))?;
        knowledge_set(cli, namespace, &knowledge_path, content).await?;
        synced_paths.push(knowledge_path);
    }

    let unsynced_existing = existing_paths
        .into_iter()
        .filter(|path| !synced_paths.iter().any(|synced| synced == path))
        .collect::<Vec<_>>();

    Ok((synced_paths.len(), unsynced_existing))
}
