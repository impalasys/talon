from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse
import uuid
import time
import json
import asyncio

app = FastAPI()

TOOL_TRIGGER = "lookup docs.example.com"
TOOL_CALL_ID = "call_knowledge_search_1"
TOOL_NAME = "knowledge_search"
BLOCKING_TOOL_TRIGGER = "blocking lookup docs.example.com"
BLOCKING_TOOL_CALL_ID = "call_blocking_lookup_1"
BLOCKING_TOOL_NAME = "mcp_durable_slow_blocking_lookup"
MCP_REMOTE_TOOL_NAME = "blocking_lookup"
TOOL_PREFACE = "Let me check that. "
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
}


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
        }
    )


def last_message(messages):
    return messages[-1] if messages else {}


def last_message_text(messages):
    message = last_message(messages)
    content = message.get("content", "")
    return content if isinstance(content, str) else ""


def should_emit_tool_call(messages, tools):
    return bool(tools) and TOOL_TRIGGER in last_message_text(messages).lower()


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
    }


def tool_call_response_payload(model, *, tool_call_id, tool_name, arguments):
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
                    "content": TOOL_PREFACE,
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


def build_tool_call_response(model):
    return tool_call_response_payload(
        model,
        tool_call_id=TOOL_CALL_ID,
        tool_name=TOOL_NAME,
        arguments={"query": "docs.example.com"},
    )


def build_blocking_tool_call_response(model):
    return tool_call_response_payload(
        model,
        tool_call_id=BLOCKING_TOOL_CALL_ID,
        tool_name=BLOCKING_TOOL_NAME,
        arguments={"query": "docs.example.com"},
    )


async def stream_tool_call_response(
    model, *, tool_call_id=TOOL_CALL_ID, tool_name=TOOL_NAME, arguments=None
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
                "delta": {"content": TOOL_PREFACE},
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
    last_message = last_message_text(messages)
    model = data.get("model", "minimax-m2.7")

    # Generate a deterministic reply based on input or just static
    if should_emit_blocking_tool_call(messages, tools):
        reply = None
    elif should_emit_tool_call(messages, tools):
        reply = None
    elif is_tool_followup(messages):
        if any(
            message.get("tool_call_id") == BLOCKING_TOOL_CALL_ID for message in messages
        ):
            reply = "I checked blocking_lookup for docs.example.com."
        else:
            reply = "I checked knowledge_search for docs.example.com."
    elif "square root of 144" in last_message.lower():
        reply = "The square root of 144 is 12."
    elif "hello" in last_message.lower():
        reply = "Hello! I am a mock LLM. How can I assist you today?"
    else:
        reply = "I received your message: " + last_message

    if data.get("stream", False):
        async def stream_generator():
            if should_emit_blocking_tool_call(messages, tools):
                async for chunk in stream_tool_call_response(
                    model,
                    tool_call_id=BLOCKING_TOOL_CALL_ID,
                    tool_name=BLOCKING_TOOL_NAME,
                    arguments={"query": "docs.example.com"},
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

    if should_emit_blocking_tool_call(messages, tools):
        return JSONResponse(content=build_blocking_tool_call_response(model))

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
                            "text": "blocking_lookup result for docs.example.com",
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
