"""Exercise the packaged ACP agents as the image's unprivileged default user."""
import json
import os
from pathlib import Path
import pwd
import selectors
import shutil
import ssl
import subprocess
import tempfile
import time


def initialize(command):
    request = {
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": 1, "clientCapabilities": {},
                   "clientInfo": {"name": "talon-image-smoke", "version": "1"}},
    }
    with tempfile.TemporaryFile() as errors:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors)
        try:
            process.stdin.write((json.dumps(request) + "\n").encode())
            process.stdin.flush()
            pending = b""
            deadline = time.monotonic() + 90
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                while time.monotonic() < deadline:
                    if not selector.select(timeout=1):
                        continue
                    chunk = os.read(process.stdout.fileno(), 65536)
                    if not chunk:
                        break
                    pending += chunk
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        if not line.strip():
                            continue
                        response = json.loads(line)
                        if response.get("id") != 1:
                            continue
                        assert "error" not in response, response
                        assert response["result"]["protocolVersion"] == 1, response
                        print("ACP initialize passed:", " ".join(command), flush=True)
                        return
            errors.seek(0)
            raise AssertionError(f"{command} did not initialize: {errors.read().decode(errors='replace')[-4000:]}")
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


assert os.getuid() == 1001 and os.getgid() == 1001
assert pwd.getpwuid(os.getuid()).pw_name == "codex"
assert Path.cwd() == Path("/workspace")
for directory in ["/workspace", os.environ["HOME"], os.environ["CODEX_HOME"],
                  os.environ["CLAUDE_CONFIG_DIR"], os.environ["OPENCODE_CONFIG_DIR"]]:
    with tempfile.NamedTemporaryFile(dir=directory):
        pass
assert len(ssl.create_default_context().get_ca_certs()) > 100
for command in ["bash", "curl", "git", "jq", "ssh", "ps", "python3", "pip3", "rg", "gcc", "g++", "make", "node", "npm"]:
    assert shutil.which(command), command
for command in [["codex", "--version"], ["claude", "--version"], ["opencode", "--version"], ["pip3", "--version"]]:
    subprocess.run(command, check=True, timeout=30)
with tempfile.TemporaryDirectory(dir="/workspace") as directory:
    for compiler, extension in [("gcc", "c"), ("g++", "cpp")]:
        source = Path(directory) / ("hello." + extension)
        source.write_text("int main() { return 0; }\n")
        binary = Path(directory) / ("hello-" + extension)
        subprocess.run([compiler, str(source), "-o", str(binary)], check=True, timeout=30)
        subprocess.run([str(binary)], check=True, timeout=10)
for command in [["codex-acp"], ["claude-code-acp"], ["opencode", "acp"]]:
    initialize(command)
print("ACP image smoke checks passed", flush=True)
