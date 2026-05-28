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

## Structured storage columns

The storage API passes a structured key instead of a pre-rendered string:

```text
namespace, parent_path, kind, name
```

SQL backends store those fields as separate text columns with the value as protobuf bytes:

```sql
PRIMARY KEY (namespace, parent_path, kind, name)
```

For Postgres this avoids prefix scans for direct children:

```sql
WHERE namespace = $1 AND parent_path = $2 AND kind = $3
```

For SQLite the same key is stored in a `WITHOUT ROWID` table so the primary-key B-tree is the table storage. This keeps sibling resources clustered by `namespace`, `parent_path`, `kind`, and `name`.

## Canonical debug format

The canonical string form is still used for debugging, documentation, and migrating older tables:

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

Direct children are listed by exact `namespace`, `parent_path`, and optional `kind`. For example, sessions directly under an agent use:

```text
namespace = Impala:Talon
parent_path = Agent/hello-agent
kind = Session
```

It does not include session messages, because messages are stored under the session parent path:

```text
parent_path = Agent/hello-agent/Session/01KSNZ13X37KYK7TNWQH7C3XPG
kind = SessionMessage
```

Recursive operations walk direct children in application code. Talon does not maintain a second recursive index in the SQL storage layer.

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
