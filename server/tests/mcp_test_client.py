from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urlparse
from urllib.request import url2pathname

from mcp.shared.version import LATEST_PROTOCOL_VERSION


class McpProcessClient:
    def __init__(
        self,
        command: list[str],
        *,
        cwd: Path,
        env: dict[str, str] | None = None,
        root_uris: list[str] | None = None,
    ) -> None:
        self._command = command
        self._cwd = cwd
        self._env = env or {}
        self._root_uris = root_uris
        self._process: subprocess.Popen[str] | None = None
        self._request_id = 0

    def start(self) -> None:
        if self._process is not None:
            return

        merged_env = os.environ.copy()
        merged_env.update(self._env)
        self._process = subprocess.Popen(
            self._command,
            cwd=self._cwd,
            env=merged_env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

    def initialize(self) -> dict[str, Any]:
        response = self.request(
            "initialize",
            {
                "protocolVersion": LATEST_PROTOCOL_VERSION,
                "capabilities": self._client_capabilities(),
                "clientInfo": {
                    "name": "dwg-mcp-test-client",
                    "version": "0.1.0",
                },
            },
        )
        self.notify("notifications/initialized", {})
        return response

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        self.start()
        assert self._process is not None
        assert self._process.stdin is not None
        assert self._process.stdout is not None

        self._request_id += 1
        message = {
            "jsonrpc": "2.0",
            "id": self._request_id,
            "method": method,
        }
        if params is not None:
            message["params"] = params

        self._process.stdin.write(json.dumps(message) + "\n")
        self._process.stdin.flush()

        while True:
            line = self._process.stdout.readline()
            if not line:
                raise RuntimeError(
                    f"server closed stdout unexpectedly\nstderr:\n{self.stderr_text()}"
                )

            message = json.loads(line)
            if "method" in message:
                self._handle_server_message(message)
                continue

            if message.get("id") != self._request_id:
                raise RuntimeError(f"response id mismatch: {message}")
            return message

    def notify(self, method: str, params: dict[str, Any] | None = None) -> None:
        self.start()
        assert self._process is not None
        assert self._process.stdin is not None

        message = {
            "jsonrpc": "2.0",
            "method": method,
        }
        if params is not None:
            message["params"] = params

        self._process.stdin.write(json.dumps(message) + "\n")
        self._process.stdin.flush()

    def terminate(self) -> None:
        process = self._process
        if process is None:
            return

        if process.stdin is not None:
            process.stdin.close()

        try:
            process.terminate()
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        finally:
            if process.stdout is not None:
                process.stdout.close()
            if process.stderr is not None:
                process.stderr.close()
            self._process = None

    def stderr_text(self) -> str:
        if self._process is None or self._process.stderr is None:
            return ""

        return self._process.stderr.read()

    def _client_capabilities(self) -> dict[str, Any]:
        if self._root_uris is None:
            return {}
        return {
            "roots": {
                "listChanged": False,
            }
        }

    def _handle_server_message(self, message: dict[str, Any]) -> None:
        request_id = message.get("id")
        method = message.get("method")
        if request_id is None:
            return

        if method == "roots/list":
            self._send_response(
                request_id,
                {
                    "roots": [
                        {
                            "uri": root_uri,
                            "name": self._root_name(root_uri),
                        }
                        for root_uri in (self._root_uris or [])
                    ]
                },
            )
            return

        self._send_error(request_id, -32601, f"unsupported server request: {method}")

    def _send_response(self, request_id: int | str, result: dict[str, Any]) -> None:
        assert self._process is not None
        assert self._process.stdin is not None
        self._process.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": result,
                }
            )
            + "\n"
        )
        self._process.stdin.flush()

    def _send_error(self, request_id: int | str, code: int, message: str) -> None:
        assert self._process is not None
        assert self._process.stdin is not None
        self._process.stdin.write(
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": code,
                        "message": message,
                    },
                }
            )
            + "\n"
        )
        self._process.stdin.flush()

    @staticmethod
    def _root_name(root_uri: str) -> str:
        parsed = urlparse(root_uri)
        path = Path(url2pathname(unquote(parsed.path or "")))
        if path.name:
            return path.name
        return root_uri
