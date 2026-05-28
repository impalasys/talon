# KV Storage Key Format

Namespaces are organized in their own hierarchy. They are resources, but they are not listed as children in the ordinary resource hierarchy inside a namespace.

## Canonical resources

Most persisted Talon objects are resources inside an isolated namespace. Namespace names are defined in a colon-separated hierarchy, for example `Impala:Talon`.

Each resource is referred to by a `<Type>/<Name>` pair. `Name` is the resource's stable Talon name in that context, not necessarily an opaque database ID. For example, agents use `Agent/<agent-name>`, while sessions and messages commonly use generated IDs as their names.

Resource paths are nested by appending more pairs. Resource names are percent-encoded before serialization so `/` remains a structural separator:

```text
<Type>/<Name>/<Type>/<Name>
```

For example:

```text
Agent/hello-agent/Session/01KSNZ13X37KYK7TNWQH7C3XPG
```

## Key serialization format

Stored keys use this shape:

```text
@Namespace/<namespace>/<ParentType>/<parent-name>/.../@/<ChildType>/<child-name>
```

For example:

```text
@Namespace/Impala:Talon/Agent/hello-agent/Session/01KSNZ13X37KYK7TNWQH7C3XPG/@/SessionMessage/01KSNZ1BAQ8GGHV75342GHSGE4
```

The segment before `/@/` identifies the namespace and parent resource path:

```text
@Namespace/Impala:Talon/Agent/hello-agent/Session/01KSNZ13X37KYK7TNWQH7C3XPG
```

The segment after `/@/` identifies the direct child resource stored at that key:

```text
SessionMessage/01KSNZ1BAQ8GGHV75342GHSGE4
```

Root-level resources in a namespace use the namespace itself as the parent path:

```text
@Namespace/Impala:Talon/@/Agent/hello-agent
```

## Listing behavior

This format allows direct children of a resource to be listed with a single ordered-KV prefix scan. For example, this prefix lists sessions directly under the agent:

```text
@Namespace/Impala:Talon/Agent/hello-agent/@/
```

It does not include session messages, because messages are stored under the session parent path:

```text
@Namespace/Impala:Talon/Agent/hello-agent/Session/01KSNZ13X37KYK7TNWQH7C3XPG/@/SessionMessage/...
```

To recursively list all resources under an agent, query the parent path prefix:

```text
@Namespace/Impala:Talon/Agent/hello-agent/
```

The `/@/` separator is intentionally only placed immediately before the leaf resource. That keeps direct-child listing distinct from recursive descendant listing without requiring a second index.

## Namespace hierarchy

Namespace hierarchies are not represented in the ordinary resource hierarchy. A separate `NamespaceRef` resource defines each child namespace edge:

```text
@Namespace/Impala/@/NamespaceRef/Talon
```

This implies the existence of an `Impala:Talon` namespace.

Root namespace edges are stored as system resources:

```text
@Namespace/Sys/@/NamespaceRef/Impala
```

## System namespace

The special namespace `Sys` is reserved for system and global resources such as namespace metadata and MCP servers. For example, MCP server keys use:

```text
@Namespace/Sys/@/MCPServer/github
```
