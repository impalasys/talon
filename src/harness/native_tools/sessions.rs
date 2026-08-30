use super::*;

pub(crate) async fn read_session_messages(
    cp: &ControlPlane,
    current_namespace: &str,
    current_agent: &str,
    args: &Value,
) -> Result<String> {
    let namespace = opt_str(args, "namespace").unwrap_or(current_namespace);
    let agent = opt_str(args, "agent").unwrap_or(current_agent);
    let session_id = req_str(args, "session_id")?;
    let limit = opt_usize(args, "limit").unwrap_or(20).clamp(1, 100);
    let mut entries = cp
        .kv
        .list_entries(
            &keys::session_message_prefix(namespace, agent, session_id),
            Some(ListOptions::desc().limit(limit)),
        )
        .await?;
    let mut messages = Vec::new();
    entries.reverse();
    for (_, bytes) in entries {
        let message = data_proto::SessionMessage::decode(bytes.as_slice())?;
        let role = data_proto::MessageRole::try_from(message.role)
            .map(|role| format!("{role:?}"))
            .unwrap_or_else(|_| message.role.to_string());
        let text = message
            .parts
            .iter()
            .filter(|part| part.part_type == data_proto::SessionMessagePartType::Text as i32)
            .map(|part| part.content.as_str())
            .collect::<String>();
        messages.push(json!({
            "id": message.id,
            "role": role,
            "text": text,
            "createdAt": message.created_at,
            "labels": message.labels,
        }));
    }
    Ok(serde_json::to_string_pretty(&json!({
        "namespace": namespace,
        "agent": agent,
        "sessionId": session_id,
        "messages": messages,
    }))?)
}
