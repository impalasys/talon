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
DEFAULT_REASONING = [
    "Inspecting the request.",
    "Planning a concise answer.",
]


def last_message(messages):
    return messages[-1] if messages else {}


def last_message_text(messages):
    message = last_message(messages)
    content = message.get("content", "")
    return content if isinstance(content, str) else ""


def should_emit_tool_call(messages, tools):
    return bool(tools) and TOOL_TRIGGER in last_message_text(messages).lower()


def is_tool_followup(messages):
    message = last_message(messages)
    return message.get("role") == "tool" and message.get("tool_call_id") == TOOL_CALL_ID


def build_tool_call_response(model):
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
                    "content": "",
                    "tool_calls": [
                        {
                            "id": TOOL_CALL_ID,
                            "type": "function",
                            "function": {
                                "name": TOOL_NAME,
                                "arguments": json.dumps({"query": "docs.example.com"}),
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


async def stream_tool_call_response(model):
    tool_call_chunks = [
        {
            "index": 0,
            "id": TOOL_CALL_ID,
            "type": "function",
            "function": {
                "name": TOOL_NAME,
                "arguments": "",
            },
        },
        {
            "index": 0,
            "function": {
                "arguments": json.dumps({"query": "docs.example.com"}),
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
    
    # Very rudimentary context extraction to mock a response
    messages = data.get("messages", [])
    tools = data.get("tools", [])
    last_message = last_message_text(messages)
    model = data.get("model", "minimax-m2.7")

    # Generate a deterministic reply based on input or just static
    if should_emit_tool_call(messages, tools):
        reply = None
    elif is_tool_followup(messages):
        reply = "I checked knowledge_search for docs.example.com."
    elif "square root of 144" in last_message.lower():
        reply = "The square root of 144 is 12."
    elif "hello" in last_message.lower():
        reply = "Hello! I am a mock LLM. How can I assist you today?"
    else:
        reply = "I received your message: " + last_message

    if data.get("stream", False):
        async def stream_generator():
            if should_emit_tool_call(messages, tools):
                async for chunk in stream_tool_call_response(model):
                    yield chunk
                return

            async for chunk in stream_text_response(model, reply):
                yield chunk
            
        return StreamingResponse(stream_generator(), media_type="text/event-stream")

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

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
