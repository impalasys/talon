from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, StreamingResponse
import uuid
import time
import json
import asyncio

app = FastAPI()

# A very crude endpoint to mock OpenAI structured chat completion
@app.post("/chat/completions")
async def chat_completions(request: Request):
    data = await request.json()
    
    # Very rudimentary context extraction to mock a response
    messages = data.get("messages", [])
    last_message = messages[-1]["content"] if messages else ""

    # Generate a deterministic reply based on input or just static
    if "square root of 144" in last_message.lower():
        reply = "The square root of 144 is 12."
    elif "hello" in last_message.lower():
        reply = "Hello! I am a mock LLM. How can I assist you today?"
    else:
        reply = "I received your message: " + last_message

    if data.get("stream", False):
        async def stream_generator():
            # Mock generating tokens by splitting the reply into words
            words = reply.split()
            for i, word in enumerate(words):
                chunk = word + (" " if i < len(words) - 1 else "")
                response_chunk = {
                    "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
                    "object": "chat.completion.chunk",
                    "created": int(time.time()),
                    "model": data.get("model", "minimax-m2.7"),
                    "choices": [
                        {
                            "index": 0,
                            "delta": {"content": chunk},
                            "finish_reason": None
                        }
                    ]
                }
                yield f"data: {json.dumps(response_chunk)}\n\n"
                await asyncio.sleep(0.1) # Small delay to simulate streaming
                
            # Final chunk with finish_reason
            final_chunk = {
                "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
                "object": "chat.completion.chunk",
                "created": int(time.time()),
                "model": data.get("model", "minimax-m2.7"),
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
            
        return StreamingResponse(stream_generator(), media_type="text/event-stream")

    response_json = {
        "id": f"chatcmpl-{uuid.uuid4().hex[:8]}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": data.get("model", "minimax-m2.7"),
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
            "total_tokens": 20
        }
    }
    return JSONResponse(content=response_json)

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
