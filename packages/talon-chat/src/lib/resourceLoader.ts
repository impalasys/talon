import { parseResourceUri, type ResourceViewModel } from "./resourceUris";
import { loadSignedInlineContent } from "./resourceSignedContent";
import type { TalonClient } from "@impalasys/talon-client";

type ResourceGatewayClient = {
  artifacts?: Pick<TalonClient["artifacts"], "readArtifact">;
  files?: Pick<TalonClient["files"], "readFile">;
};

const callerAgentHeader = "x-talon-agent";
const callerSessionHeader = "x-talon-session-id";
const base64Content = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|(?:[A-Za-z0-9+/]{3}=))?$/;

function requestHeaders(agent: string, sessionId: string | null) {
  const headers: Record<string, string> = {};
  if (agent) headers[callerAgentHeader] = agent;
  if (sessionId) headers[callerSessionHeader] = sessionId;
  return Object.keys(headers).length > 0 ? headers : undefined;
}

function contentBytes(content: unknown): Uint8Array | undefined {
  if (content == null) return undefined;
  if (content instanceof Uint8Array) return content;
  if (typeof content === "string") {
    if (content.length === 0) return new Uint8Array(0);
    if (content.length >= 16 && content.length % 4 === 0 && base64Content.test(content)) {
      try {
        const binary = atob(content);
        return Uint8Array.from(binary, (character) => character.charCodeAt(0));
      } catch { /* Treat invalid base64-shaped content as UTF-8. */ }
    }
    return new TextEncoder().encode(content);
  }
  return ArrayBuffer.isView(content)
    ? new Uint8Array(content.buffer, content.byteOffset, content.byteLength)
    : undefined;
}

/** Load the binary or signed-URL payload behind a session resource URI. */
export async function fetchResourceFromGateway({
  uri, gatewayClient, agent, sessionId, signal,
}: {
  uri: string;
  gatewayClient: ResourceGatewayClient;
  agent: string;
  sessionId: string | null;
  signal: AbortSignal;
}): Promise<ResourceViewModel> {
  const parsed = parseResourceUri(uri);
  if (!parsed) throw new Error(`Unsupported resource URI: ${uri}`);
  const headers = requestHeaders(agent, sessionId);
  const options = { signal, ...(headers ? { headers } : {}) };
  if (parsed.kind === "artifact") {
    const artifacts = gatewayClient.artifacts;
    if (!artifacts?.readArtifact) throw new Error("Gateway client does not expose artifacts.readArtifact().");
    const response = await (artifacts.readArtifact as any)({ artifactUri: parsed.uri }, options);
    const artifact = response?.artifact ?? {};
    const mediaType = artifact.mediaType || artifact.media_type || "application/octet-stream";
    const signedUrl = response?.signedUrl || response?.signed_url || undefined;
    const content = await loadSignedInlineContent({
      content: contentBytes(response?.content),
      mediaType,
      signedUrl,
      sizeBytes: artifact.objectRef?.sizeBytes ?? artifact.object_ref?.size_bytes,
      signal,
    });
    return {
      kind: "artifact", uri: parsed.uri,
      title: (typeof artifact.title === "string" && artifact.title) || parsed.artifactId,
      mediaType,
      content, signedUrl,
      objectKey: typeof artifact.objectRef?.key === "string"
        ? artifact.objectRef.key
        : typeof artifact.object_ref?.key === "string"
          ? artifact.object_ref.key
          : undefined,
      sessionId: parsed.sessionId, agent: parsed.agent,
    };
  }
  const files = gatewayClient.files;
  if (!files?.readFile) throw new Error("Gateway client does not expose files.readFile().");
  const response = await (files.readFile as any)({ file: { uri: parsed.uri } }, options);
  const file = response?.file ?? {};
  const metadata = file.metadata ?? {};
  const spec = file.spec ?? {};
  return {
    kind: "file", uri: parsed.uri,
    title: (typeof metadata.name === "string" && metadata.name) || (typeof spec.path === "string" && spec.path.split("/").filter(Boolean).pop()) || parsed.fileName,
    mediaType: spec.mediaType || spec.media_type || "application/octet-stream",
    content: contentBytes(response?.content), signedUrl: response?.signedUrl || response?.signed_url || undefined,
    path: typeof spec.path === "string" ? spec.path : undefined,
  };
}
