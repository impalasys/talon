from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse
import uuid
import time
import json
import asyncio
from pathlib import Path
import re

app = FastAPI()

TOOL_TRIGGER = "lookup docs.example.com"
TOOL_CALL_ID = "call_knowledge_search_1"
TOOL_NAME = "knowledge_search"
DELEGATE_TASK_TRIGGER = "delegate onboarding task"
DELEGATE_TASK_CALL_ID = "call_delegate_task_1"
DELEGATE_TASK_NAME = "delegate_task"
CREATE_ARTIFACT_CALL_ID = "call_create_artifact_1"
CREATE_ARTIFACT_NAME = "create_artifact"
READ_ARTIFACT_CALL_ID = "call_read_artifact_1"
READ_ARTIFACT_NAME = "read_artifact"
SCENARIO_DIR = Path(__file__).resolve().parent / "fixtures" / "mock_llm_scenarios"
BLOCKING_TOOL_TRIGGER = "blocking lookup docs.example.com"
BLOCKING_TOOL_CALL_ID = "call_blocking_lookup_1"
BLOCKING_TOOL_NAME = "mcp_durable_slow_blocking_lookup"
MCP_REMOTE_TOOL_NAME = "blocking_lookup"
TOOL_PREFACE = "Let me check that. "
LARGE_MCP_TOOL_RESULT = (
    "blocking_lookup result for docs.example.com\n"
    + "\n".join(
        f"docs.example.com reference section {idx:03d}: deterministic content for CAS hydration."
        for idx in range(80)
    )
)
SUPER_LARGE_MCP_TOOL_RESULT = (
    "blocking_lookup result for super-large-docs.example.com\n"
    + "\n".join(
        f"super-large-docs.example.com reference section {idx:05d}: "
        f"{uuid.uuid5(uuid.NAMESPACE_DNS, f'super-large-docs.example.com:{idx}')}"
        for idx in range(16_000)
    )
)
DEFAULT_REASONING = [
    "Inspecting the request.",
    "Planning a concise answer.",
]
CONTROL_STATE = {
    "block_after_chunks": None,
    "blocked": False,
    "unblocked": False,
    "request_count": 0,
    "stream_text_chunks": 0,
    "block_mcp_tool": False,
    "mcp_tool_blocked": False,
    "mcp_tool_unblocked": False,
    "mcp_tool_call_count": 0,
    "chat_requests": [],
}


def load_mock_scenarios():
    scenarios = []
    if not SCENARIO_DIR.exists():
        return scenarios
    for path in sorted(SCENARIO_DIR.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        for rule in data.get("rules", []):
            rule["_source"] = path.name
            scenarios.append(rule)
    return scenarios


MOCK_SCENARIO_RULES = load_mock_scenarios()


def reset_control_state():
    CONTROL_STATE.update(
        {
            "block_after_chunks": None,
            "blocked": False,
            "unblocked": False,
            "request_count": 0,
            "stream_text_chunks": 0,
            "block_mcp_tool": False,
            "mcp_tool_blocked": False,
            "mcp_tool_unblocked": False,
            "mcp_tool_call_count": 0,
            "chat_requests": [],
        }
    )


def last_message(messages):
    return messages[-1] if messages else {}


def last_message_text(messages):
    message = last_message(messages)
    content = message.get("content", "")
    return content if isinstance(content, str) else ""


def system_message_text(messages):
    return "\n".join(
        message.get("content", "")
        for message in messages
        if message.get("role") == "system" and isinstance(message.get("content"), str)
    )


def available_tool_names(tools):
    return {
        tool.get("function", {}).get("name")
        for tool in tools
        if isinstance(tool, dict)
    }


def should_emit_tool_call(messages, tools):
    return bool(tools) and TOOL_TRIGGER in last_message_text(messages).lower()


def collect_scenario_vars(rule, messages):
    text = last_message_text(messages)
    values = {"artifact_uri": artifact_uri_from_text(text)}
    for capture in rule.get("captures", []):
        match = re.search(capture["pattern"], text, re.IGNORECASE)
        if match:
            values[capture["name"]] = match.group(1)
    return values


def render_scenario_value(value, variables):
    if isinstance(value, str):
        rendered = value
        for key, replacement in variables.items():
            rendered = rendered.replace(f"{{{{{key}}}}}", replacement)
        return rendered
    if isinstance(value, list):
        return [render_scenario_value(item, variables) for item in value]
    if isinstance(value, dict):
        return {
            key: render_scenario_value(item, variables)
            for key, item in value.items()
        }
    return value


def scenario_response_for(messages, tools):
    system_text = system_message_text(messages).lower()
    last_text = last_message_text(messages).lower()
    tool_names = available_tool_names(tools)
    last = last_message(messages)
    for rule in MOCK_SCENARIO_RULES:
        expected_tool_call_id = rule.get("tool_call_id")
        if expected_tool_call_id:
            if last.get("role") != "tool" or last.get("tool_call_id") != expected_tool_call_id:
                continue
        elif last.get("role") == "tool":
            continue

        system_contains = rule.get("system_contains")
        if system_contains and system_contains.lower() not in system_text:
            continue
        last_contains = rule.get("last_message_contains")
        if last_contains and last_contains.lower() not in last_text:
            continue
        tool_name = rule.get("tool_name")
        if tool_name and tool_name not in tool_names:
            continue

        response = dict(rule["response"])
        variables = collect_scenario_vars(rule, messages)
        response["arguments"] = render_scenario_value(
            response.get("arguments", {}),
            variables,
        )
        return response
    return None


def should_emit_delegate_task_call(messages, tools):
    tool_names = {
        tool.get("function", {}).get("name")
        for tool in tools
        if isinstance(tool, dict)
    }
    return (
        DELEGATE_TASK_NAME in tool_names
        and DELEGATE_TASK_TRIGGER in last_message_text(messages).lower()
    )


def should_emit_delegated_artifact_call(messages, tools):
    tool_names = {
        tool.get("function", {}).get("name")
        for tool in tools
        if isinstance(tool, dict)
    }
    return (
        CREATE_ARTIFACT_NAME in tool_names
        and "you have been assigned a talon task" in last_message_text(messages).lower()
    )


def artifact_uri_from_text(text):
    match = re.search(r"artifact://[^\s'\")]+", text)
    return match.group(0).strip(".,;)") if match else ""


def should_emit_read_artifact_call(messages, tools):
    tool_names = {
        tool.get("function", {}).get("name")
        for tool in tools
        if isinstance(tool, dict)
    }
    text = last_message_text(messages).lower()
    return (
        READ_ARTIFACT_NAME in tool_names
        and ("read this" in text or "read artifact" in text)
        and artifact_uri_from_text(text)
    )


def should_emit_blocking_tool_call(messages, tools):
    tool_names = {
        tool.get("function", {}).get("name")
        for tool in tools
        if isinstance(tool, dict)
    }
    return (
        BLOCKING_TOOL_NAME in tool_names
        and BLOCKING_TOOL_TRIGGER in last_message_text(messages).lower()
    )


def is_tool_followup(messages):
    message = last_message(messages)
    return message.get("role") == "tool" and message.get("tool_call_id") in {
        TOOL_CALL_ID,
        BLOCKING_TOOL_CALL_ID,
        DELEGATE_TASK_CALL_ID,
        CREATE_ARTIFACT_CALL_ID,
        READ_ARTIFACT_CALL_ID,
    }


def tool_call_response_payload(
    model,
    *,
    tool_call_id,
    tool_name,
    arguments,
    content=TOOL_PREFACE,
):
    return {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content,
                    "tool_calls": [
                        {
                            "id": tool_call_id,
                            "type": "function",
                            "function": {
                                "name": tool_name,
                                "arguments": json.dumps(arguments),
                            },
                        }
                    ],
                },
                "finish_reason": "tool_calls",
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 10,
            "total_tokens": 20,
        },
    }


def scenario_tool_call_payload(model, response):
    return tool_call_response_payload(
        model,
        tool_call_id=response["tool_call_id"],
        tool_name=response["tool_name"],
        arguments=response.get("arguments", {}),
        content=response.get("content", TOOL_PREFACE),
    )


def build_tool_call_response(model):
    return tool_call_response_payload(
        model,
        tool_call_id=TOOL_CALL_ID,
        tool_name=TOOL_NAME,
        arguments={"query": "docs.example.com"},
    )


def build_blocking_tool_call_response(model, query="docs.example.com"):
    return tool_call_response_payload(
        model,
        tool_call_id=BLOCKING_TOOL_CALL_ID,
        tool_name=BLOCKING_TOOL_NAME,
        arguments={"query": query},
    )


def delegate_task_arguments(text):
    return {
        "title": "Prepare customer onboarding checklist",
        "description": "Create a reviewed onboarding checklist.",
        "type": "OPERATIONS",
        "connection": "worker",
    }


def build_delegate_task_call_response(model, messages):
    return tool_call_response_payload(
        model,
        tool_call_id=DELEGATE_TASK_CALL_ID,
        tool_name=DELEGATE_TASK_NAME,
        arguments=delegate_task_arguments(last_message_text(messages)),
    )


def artifact_arguments_for_task(text):
    return {
        "title": "Onboarding checklist artifact",
        "content": "# Onboarding checklist\n\n- Confirm kickoff owner\n- Prepare success plan",
        "media_type": "text/markdown",
        "metadata": {"source": "delegated-worker"},
    }


def build_create_artifact_call_response(model, messages):
    return tool_call_response_payload(
        model,
        tool_call_id=CREATE_ARTIFACT_CALL_ID,
        tool_name=CREATE_ARTIFACT_NAME,
        arguments=artifact_arguments_for_task(last_message_text(messages)),
    )


def build_read_artifact_call_response(model, artifact_uri):
    return tool_call_response_payload(
        model,
        tool_call_id=READ_ARTIFACT_CALL_ID,
        tool_name=READ_ARTIFACT_NAME,
        arguments={"artifact_uri": artifact_uri},
    )


async def stream_tool_call_response(
    model,
    *,
    tool_call_id=TOOL_CALL_ID,
    tool_name=TOOL_NAME,
    arguments=None,
    content=TOOL_PREFACE,
):
    arguments = arguments or {"query": "docs.example.com"}
    preface_chunk = {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [
            {
                "index": 0,
                "delta": {"content": content},
                "finish_reason": None,
            }
        ],
    }
    yield f"data: {json.dumps(preface_chunk)}\n\n"
    await asyncio.sleep(0.05)

    tool_call_chunks = [
        {
            "index": 0,
            "id": tool_call_id,
            "type": "function",
            "function": {
                "name": tool_name,
                "arguments": "",
            },
        },
        {
            "index": 0,
            "function": {
                "arguments": json.dumps(arguments),
            },
        },
    ]
    for chunk in tool_call_chunks:
        response_chunk = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "delta": {"tool_calls": [chunk]},
                    "finish_reason": None,
                }
            ],
        }
        yield f"data: {json.dumps(response_chunk)}\n\n"
        await asyncio.sleep(0.05)

    final_chunk = {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "model": model,
        "choices": [
            {
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls",
            }
        ],
    }
    yield f"data: {json.dumps(final_chunk)}\n\n"
    yield "data: [DONE]\n\n"


async def stream_scenario_response(model, response):
    if response.get("type") == "tool_call":
        async for chunk in stream_tool_call_response(
            model,
            tool_call_id=response["tool_call_id"],
            tool_name=response["tool_name"],
            arguments=response.get("arguments", {}),
            content=response.get("content", TOOL_PREFACE),
        ):
            yield chunk
        return

    async for chunk in stream_text_response(model, response.get("content", "")):
        yield chunk


async def stream_text_response(model, reply):
    for reasoning in DEFAULT_REASONING:
        reasoning_chunk = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "delta": {"reasoning": reasoning + " "},
                    "finish_reason": None
                }
            ]
        }
        yield f"data: {json.dumps(reasoning_chunk)}\n\n"
        await asyncio.sleep(0.05)

    words = reply.split()
    for i, word in enumerate(words):
        chunk = word + (" " if i < len(words) - 1 else "")
        response_chunk = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "delta": {"content": chunk},
                    "finish_reason": None
                }
            ]
        }
        yield f"data: {json.dumps(response_chunk)}\n\n"
        CONTROL_STATE["stream_text_chunks"] += 1
        block_after = CONTROL_STATE["block_after_chunks"]
        if (
            block_after is not None
            and not CONTROL_STATE["unblocked"]
            and CONTROL_STATE["stream_text_chunks"] >= block_after
        ):
            CONTROL_STATE["blocked"] = True
            while not CONTROL_STATE["unblocked"]:
                await asyncio.sleep(0.05)
        await asyncio.sleep(0.1)

    final_chunk = {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion.chunk",
        "created": int(time.time()),
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 10,
            "reasoning_tokens": 6,
            "total_tokens": 26
        },
        "model": model,
        "choices": [
            {
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }
        ]
    }
    yield f"data: {json.dumps(final_chunk)}\n\n"
    yield "data: [DONE]\n\n"

# A very crude endpoint to mock OpenAI structured chat completion
@app.post("/chat/completions")
async def chat_completions(request: Request):
    data = await request.json()
    CONTROL_STATE["request_count"] += 1
    
    # Very rudimentary context extraction to mock a response
    messages = data.get("messages", [])
    tools = data.get("tools", [])
    CONTROL_STATE["chat_requests"].append(
        {
            "messages": messages,
            "stream": bool(data.get("stream", False)),
            "toolNames": [
                tool.get("function", {}).get("name")
                for tool in tools
                if isinstance(tool, dict)
            ],
        }
    )
    last_text = last_message_text(messages)
    model = data.get("model", "minimax-m2.7")
    scenario_response = scenario_response_for(messages, tools)

    # Generate a deterministic reply based on input or just static
    if scenario_response is not None:
        reply = None
    elif should_emit_blocking_tool_call(messages, tools):
        reply = None
    elif should_emit_delegate_task_call(messages, tools):
        reply = None
    elif should_emit_delegated_artifact_call(messages, tools):
        reply = None
    elif should_emit_read_artifact_call(messages, tools):
        reply = None
    elif should_emit_tool_call(messages, tools):
        reply = None
    elif is_tool_followup(messages):
        tool_call_id = last_message(messages).get("tool_call_id")
        if tool_call_id == DELEGATE_TASK_CALL_ID:
            reply = "I delegated the onboarding task."
        elif tool_call_id == CREATE_ARTIFACT_CALL_ID:
            reply = "I created the onboarding checklist artifact."
        elif tool_call_id == READ_ARTIFACT_CALL_ID:
            reply = "I read the delegated artifact."
        elif tool_call_id == BLOCKING_TOOL_CALL_ID:
            reply = "I checked blocking_lookup for docs.example.com."
        else:
            reply = "I checked knowledge_search for docs.example.com."
    elif "square root of 144" in last_text.lower():
        reply = "The square root of 144 is 12."
    elif "hello" in last_text.lower():
        reply = "Hello! I am a mock LLM. How can I assist you today?"
    else:
        reply = "I received your message: " + last_text

    if data.get("stream", False):
        async def stream_generator():
            if scenario_response is not None:
                async for chunk in stream_scenario_response(model, scenario_response):
                    yield chunk
                return

            if should_emit_blocking_tool_call(messages, tools):
                query = (
                    "super-large-docs.example.com"
                    if "super large" in last_text.lower()
                    else "docs.example.com"
                )
                async for chunk in stream_tool_call_response(
                    model,
                    tool_call_id=BLOCKING_TOOL_CALL_ID,
                    tool_name=BLOCKING_TOOL_NAME,
                    arguments={"query": query},
                ):
                    yield chunk
                return

            if should_emit_delegate_task_call(messages, tools):
                async for chunk in stream_tool_call_response(
                    model,
                    tool_call_id=DELEGATE_TASK_CALL_ID,
                    tool_name=DELEGATE_TASK_NAME,
                    arguments=delegate_task_arguments(last_message_text(messages)),
                ):
                    yield chunk
                return

            if should_emit_delegated_artifact_call(messages, tools):
                async for chunk in stream_tool_call_response(
                    model,
                    tool_call_id=CREATE_ARTIFACT_CALL_ID,
                    tool_name=CREATE_ARTIFACT_NAME,
                    arguments=artifact_arguments_for_task(last_message_text(messages)),
                ):
                    yield chunk
                return

            if should_emit_read_artifact_call(messages, tools):
                async for chunk in stream_tool_call_response(
                    model,
                    tool_call_id=READ_ARTIFACT_CALL_ID,
                    tool_name=READ_ARTIFACT_NAME,
                    arguments={
                        "artifact_uri": artifact_uri_from_text(last_text),
                    },
                ):
                    yield chunk
                return

            if should_emit_tool_call(messages, tools):
                async for chunk in stream_tool_call_response(model):
                    yield chunk
                return

            async for chunk in stream_text_response(model, reply):
                yield chunk
            
        return StreamingResponse(stream_generator(), media_type="text/event-stream")

    if scenario_response is not None:
        if scenario_response.get("type") == "tool_call":
            return JSONResponse(content=scenario_tool_call_payload(model, scenario_response))
        response_json = {
            "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": scenario_response.get("content", ""),
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 10,
                "reasoning_tokens": 6,
                "total_tokens": 26,
            },
        }
        return JSONResponse(content=response_json)

    if should_emit_blocking_tool_call(messages, tools):
        query = (
            "super-large-docs.example.com"
            if "super large" in last_text.lower()
            else "docs.example.com"
        )
        return JSONResponse(content=build_blocking_tool_call_response(model, query))

    if should_emit_delegate_task_call(messages, tools):
        return JSONResponse(content=build_delegate_task_call_response(model, messages))

    if should_emit_delegated_artifact_call(messages, tools):
        return JSONResponse(content=build_create_artifact_call_response(model, messages))

    if should_emit_read_artifact_call(messages, tools):
        return JSONResponse(
            content=build_read_artifact_call_response(
                model,
                artifact_uri_from_text(last_text),
            )
        )

    if should_emit_tool_call(messages, tools):
        return JSONResponse(content=build_tool_call_response(model))

    response_json = {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": reply
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 10,
            "reasoning_tokens": 6,
            "total_tokens": 26
        }
    }
    return JSONResponse(content=response_json)


@app.post("/__control/reset")
async def control_reset():
    reset_control_state()
    return JSONResponse(content=dict(CONTROL_STATE))


@app.post("/__control/block_stream_after_chunks")
async def control_block_stream_after_chunks(request: Request):
    data = await request.json()
    chunks = int(data.get("chunks", 1))
    CONTROL_STATE.update(
        {
            "block_after_chunks": max(1, chunks),
            "blocked": False,
            "unblocked": False,
            "stream_text_chunks": 0,
        }
    )
    return JSONResponse(content=dict(CONTROL_STATE))


@app.post("/__control/block_mcp_tool")
async def control_block_mcp_tool():
    CONTROL_STATE.update(
        {
            "block_mcp_tool": True,
            "mcp_tool_blocked": False,
            "mcp_tool_unblocked": False,
            "mcp_tool_call_count": 0,
        }
    )
    return JSONResponse(content=dict(CONTROL_STATE))


@app.post("/__control/unblock_stream")
async def control_unblock_stream():
    CONTROL_STATE["unblocked"] = True
    return JSONResponse(content=dict(CONTROL_STATE))


@app.post("/__control/unblock_mcp_tool")
async def control_unblock_mcp_tool():
    CONTROL_STATE["mcp_tool_unblocked"] = True
    return JSONResponse(content=dict(CONTROL_STATE))


@app.get("/__control/state")
async def control_state():
    return JSONResponse(content=dict(CONTROL_STATE))


@app.post("/mcp")
async def mcp_endpoint(request: Request):
    message = await request.json()
    method = message.get("method")
    request_id = message.get("id")

    if method == "initialize":
        return JSONResponse(
            content={
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "durable-slow", "version": "test"},
                },
            },
            headers={"Mcp-Session-Id": "mock-mcp-session"},
        )

    if method == "notifications/initialized":
        return JSONResponse(content={}, status_code=202)

    if method == "tools/list":
        return JSONResponse(
            content={
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "tools": [
                        {
                            "name": MCP_REMOTE_TOOL_NAME,
                            "description": "Blocking test lookup tool.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {"query": {"type": "string"}},
                                "required": ["query"],
                            },
                        }
                    ]
                },
            },
            headers={"Mcp-Session-Id": "mock-mcp-session"},
        )

    if method == "tools/call":
        params = message.get("params") or {}
        arguments = params.get("arguments") or {}
        CONTROL_STATE["mcp_tool_call_count"] += 1
        if params.get("name") != MCP_REMOTE_TOOL_NAME:
            return JSONResponse(
                content={
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32602,
                        "message": f"unknown tool {params.get('name')}",
                    },
                },
                status_code=400,
            )

        if CONTROL_STATE["block_mcp_tool"] and not CONTROL_STATE["mcp_tool_unblocked"]:
            CONTROL_STATE["mcp_tool_blocked"] = True
            while not CONTROL_STATE["mcp_tool_unblocked"]:
                await asyncio.sleep(0.05)

        return JSONResponse(
            content={
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": (
                                SUPER_LARGE_MCP_TOOL_RESULT
                                if arguments.get("query")
                                == "super-large-docs.example.com"
                                else LARGE_MCP_TOOL_RESULT
                            ),
                        }
                    ],
                    "isError": False,
                },
            },
            headers={"Mcp-Session-Id": "mock-mcp-session"},
        )

    return JSONResponse(
        content={
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": -32601, "message": f"unknown method {method}"},
        },
        status_code=404,
    )

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
