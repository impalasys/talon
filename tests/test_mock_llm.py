import json
import sys

import httpx
import mock_llm
import pytest


@pytest.fixture
def anyio_backend() -> str:
    return "asyncio"


def test_mock_llm_helper_functions_cover_message_and_tool_detection() -> None:
    # Verify the mock LLM helper functions classify messages and tool-call
    # conditions the way the e2e tests expect.
    messages = [{"role": "user", "content": "please lookup docs.example.com"}]
    assert mock_llm.last_message(messages) == messages[-1]
    assert mock_llm.last_message([]) == {}
    assert mock_llm.last_message_text(messages) == "please lookup docs.example.com"
    assert mock_llm.last_message_text([{"content": ["not", "a", "string"]}]) == (
        "not\na\nstring"
    )
    assert mock_llm.last_message_text(
        [{"content": [{"type": "text", "text": "structured text"}]}]
    ) == "structured text"
    assert mock_llm.system_message_text(
        [{"role": "system", "content": [{"type": "text", "text": "system text"}]}]
    ) == "system text"
    assert mock_llm.should_emit_tool_call(messages, [{"type": "function"}]) is True
    assert mock_llm.should_emit_tool_call(messages, []) is False
    assert mock_llm.is_tool_followup(
        [{"role": "tool", "tool_call_id": mock_llm.TOOL_CALL_ID}]
    ) is True
    assert mock_llm.is_tool_followup([{"role": "assistant"}]) is False

    response = mock_llm.build_tool_call_response("mock-model")
    tool_call = response["choices"][0]["message"]["tool_calls"][0]
    assert response["model"] == "mock-model"
    assert response["choices"][0]["message"]["content"] == mock_llm.TOOL_PREFACE
    assert tool_call["function"]["name"] == mock_llm.TOOL_NAME
    assert json.loads(tool_call["function"]["arguments"]) == {"query": "docs.example.com"}


def test_mock_llm_json_scenario_rules_emit_legal_delegate_tool_call() -> None:
    messages = [
        {
            "role": "system",
            "content": "You are a legal coordinator.",
        },
        {
            "role": "user",
            "content": (
                "Please delegate legal document refinement task to the legal "
                "reviewer connection."
            ),
        },
    ]
    response = mock_llm.scenario_response_for(
        messages,
        [{"type": "function", "function": {"name": "delegate_task"}}],
    )

    assert response is not None
    assert response["type"] == "tool_call"
    assert response["tool_name"] == "delegate_task"
    assert response["arguments"]["connection"] == "legal-reviewer"
    assert response["arguments"]["type"] == "legal_document_refinement"

    payload = mock_llm.scenario_tool_call_payload("mock-model", response)
    assert payload["choices"][0]["message"]["content"] == "Let me assign the legal review. "


@pytest.mark.anyio
async def test_mock_llm_non_streaming_blocking_lookup_honors_super_large_query() -> None:
    transport = httpx.ASGITransport(app=mock_llm.app)
    async with httpx.AsyncClient(transport=transport, base_url="http://testserver") as client:
        response = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "blocking lookup docs.example.com with super large result",
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {"name": mock_llm.BLOCKING_TOOL_NAME},
                    }
                ],
            },
        )

    assert response.status_code == 200
    tool_call = response.json()["choices"][0]["message"]["tool_calls"][0]
    arguments = json.loads(tool_call["function"]["arguments"])
    assert arguments == {"query": "super-large-docs.example.com"}


@pytest.mark.anyio
async def test_mock_llm_stream_helpers_cover_text_and_tool_chunks() -> None:
    # Verify the mock LLM stream helpers produce the expected chunk structure for
    # both plain text streaming and tool-call streaming responses.
    tool_chunks = [chunk async for chunk in mock_llm.stream_tool_call_response("mock-model")]
    assert tool_chunks[-1] == "data: [DONE]\n\n"
    assert any(mock_llm.TOOL_NAME in chunk for chunk in tool_chunks)
    assert any(mock_llm.TOOL_PREFACE in chunk for chunk in tool_chunks)

    text_chunks = [chunk async for chunk in mock_llm.stream_text_response("mock-model", "hello world")]
    assert text_chunks[-1] == "data: [DONE]\n\n"
    assert any("hello " in chunk or "world" in chunk for chunk in text_chunks)


@pytest.mark.anyio
async def test_mock_llm_chat_completions_endpoint_covers_json_and_streaming_paths() -> None:
    # Verify the mock LLM HTTP endpoint serves normal chat completions, tool-call
    # completions, and streaming completions through its ASGI interface.
    transport = httpx.ASGITransport(app=mock_llm.app)
    async with httpx.AsyncClient(transport=transport, base_url="http://testserver") as client:
        standard = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [{"role": "user", "content": "hello"}],
            },
        )
        assert standard.status_code == 200
        assert standard.json()["choices"][0]["message"]["content"] == (
            "Hello! I am a mock LLM. How can I assist you today?"
        )

        tool = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "messages": [{"role": "user", "content": "lookup docs.example.com"}],
                "tools": [{"type": "function", "function": {"name": mock_llm.TOOL_NAME}}],
            },
        )
        assert tool.status_code == 200
        tool_payload = tool.json()
        assert tool_payload["choices"][0]["message"]["tool_calls"][0]["function"]["name"] == mock_llm.TOOL_NAME

        stream = await client.post(
            "/chat/completions",
            json={
                "model": "mock-model",
                "stream": True,
                "messages": [{"role": "user", "content": "hello"}],
            },
        )
        assert stream.status_code == 200
        assert "data:" in stream.text


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
