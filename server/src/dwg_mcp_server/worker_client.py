from __future__ import annotations

import asyncio
import contextlib
import json
import os
import uuid
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class WorkerClientError(RuntimeError):
    """Raised when the Rust worker returns an error or becomes unavailable."""


class UnknownDocumentError(WorkerClientError):
    """Raised when a document id is not tracked by the session manager."""


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_worker_command() -> list[str]:
    configured = os.getenv("DWG_WORKER_BIN")
    if configured:
        return [configured]

    root = _repo_root()
    for profile in ("release", "debug"):
        for name in ("dwg-worker", "dwg-worker.exe"):
            candidate = root / "target" / profile / name
            if candidate.is_file():
                return [str(candidate)]

    raise WorkerClientError(
        "DWG_WORKER_BIN is not set and no dwg-worker binary was found under "
        f"{root / 'target/release'} or {root / 'target/debug'}. "
        "Build with: cargo build -p dwg-worker --release, "
        "or set DWG_WORKER_BIN to a dwg-worker executable."
    )


def _summarize_type_page(payload: dict[str, Any]) -> dict[str, Any]:
    items = payload.get("items", [])
    if not isinstance(items, list):
        return payload

    summarized_items = []
    for item in items:
        if not isinstance(item, dict):
            continue
        summarized = {
            "typeName": item.get("typeName"),
            "genericType": item.get("genericType"),
        }
        if item.get("description") is not None:
            summarized["description"] = item["description"]
        if item.get("aliases"):
            summarized["aliases"] = item["aliases"]
        if item.get("defaultSelect"):
            summarized["defaultSelect"] = item["defaultSelect"]
        summarized_items.append(summarized)

    return {
        "total": payload.get("total", len(summarized_items)),
        "nextCursor": payload.get("nextCursor"),
        "items": summarized_items,
    }


@dataclass(slots=True)
class WorkerCallResult:
    id: int
    result: dict[str, Any] | None


class WorkerProcess:
    def __init__(
        self,
        command: list[str] | None = None,
        cwd: Path | None = None,
    ) -> None:
        self._command = command or default_worker_command()
        self._cwd = cwd or _repo_root()
        self._process: asyncio.subprocess.Process | None = None
        self._request_id = 0
        self._rpc_lock = asyncio.Lock()
        self._stderr_task: asyncio.Task[None] | None = None
        self._stderr_tail: deque[str] = deque(maxlen=40)

    async def start(self) -> None:
        if self._process is not None:
            return

        self._process = await asyncio.create_subprocess_exec(
            *self._command,
            cwd=str(self._cwd),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            limit=32 * 1024 * 1024,
        )
        self._stderr_task = asyncio.create_task(self._drain_stderr())

    async def call(self, method: str, params: dict[str, Any] | None = None) -> WorkerCallResult:
        await self.start()
        if self._process is None or self._process.stdin is None or self._process.stdout is None:
            raise WorkerClientError("worker process did not start correctly")

        async with self._rpc_lock:
            self._request_id += 1
            request_id = self._request_id
            payload = {
                "id": request_id,
                "method": method,
                "params": params or {},
            }

            self._process.stdin.write((json.dumps(payload) + "\n").encode("utf-8"))
            await self._process.stdin.drain()

            line = await self._process.stdout.readline()
            if not line:
                raise WorkerClientError(self._exit_message("worker closed stdout unexpectedly"))

            try:
                response = json.loads(line.decode("utf-8"))
            except json.JSONDecodeError as error:
                raise WorkerClientError(
                    self._exit_message(f"worker returned invalid JSON: {error}")
                ) from error

            if response.get("id") != request_id:
                raise WorkerClientError(
                    self._exit_message(
                        f"worker response id mismatch: expected {request_id}, got {response.get('id')}"
                    )
                )

            if response.get("error"):
                error = response["error"]
                raise WorkerClientError(
                    self._exit_message(
                        f"{error.get('code', 'worker_error')}: {error.get('message', 'unknown error')}"
                    )
                )

            result = response.get("result")
            if result is not None and not isinstance(result, dict):
                raise WorkerClientError(
                    self._exit_message(f"worker result must be an object, got {type(result).__name__}")
                )

            return WorkerCallResult(id=request_id, result=result)

    async def terminate(self) -> None:
        process = self._process
        if process is None:
            return

        if process.stdin and not process.stdin.is_closing():
            process.stdin.close()
            with contextlib.suppress(Exception):
                await process.stdin.wait_closed()

        if process.returncode is None:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=5)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()

        if self._stderr_task is not None:
            with contextlib.suppress(asyncio.CancelledError):
                await self._stderr_task

        self._process = None
        self._stderr_task = None

    async def _drain_stderr(self) -> None:
        assert self._process is not None and self._process.stderr is not None
        while True:
            line = await self._process.stderr.readline()
            if not line:
                break
            text = line.decode("utf-8", errors="replace").rstrip()
            if text:
                self._stderr_tail.append(text)

    def _exit_message(self, prefix: str) -> str:
        stderr = "\n".join(self._stderr_tail).strip()
        if stderr:
            return f"{prefix}\nworker stderr:\n{stderr}"
        return prefix


class SessionManager:
    def __init__(
        self,
        worker_command: list[str] | None = None,
        worker_cwd: Path | None = None,
    ) -> None:
        self._worker_command = worker_command or default_worker_command()
        self._worker_cwd = worker_cwd or _repo_root()
        self._sessions: dict[str, WorkerProcess] = {}
        self._lock = asyncio.Lock()

    async def open_file(self, path: str) -> dict[str, Any]:
        worker = WorkerProcess(command=self._worker_command, cwd=self._worker_cwd)
        try:
            result = await worker.call("openFile", {"path": path})
        except Exception:
            await worker.terminate()
            raise

        document_id = str(uuid.uuid4())
        async with self._lock:
            self._sessions[document_id] = worker

        return {
            "documentId": document_id,
            **(result.result or {}),
        }

    async def close_file(self, document_id: str) -> dict[str, Any]:
        worker = await self._remove_session(document_id)
        try:
            result = await worker.call("closeFile", {})
        finally:
            await worker.terminate()

        return {
            "documentId": document_id,
            **(result.result or {}),
        }

    async def list_types(
        self,
        *,
        regex: str | None = None,
        limit: int | None = None,
        cursor: str | None = None,
    ) -> dict[str, Any]:
        temp_worker = WorkerProcess(command=self._worker_command, cwd=self._worker_cwd)
        try:
            result = await temp_worker.call(
                "listTypes",
                self._type_list_params(regex=regex, limit=limit, cursor=cursor),
            )
            return _summarize_type_page(result.result or {})
        finally:
            await temp_worker.terminate()

    async def list_file_types(
        self,
        document_id: str,
        *,
        regex: str | None = None,
        limit: int | None = None,
        cursor: str | None = None,
    ) -> dict[str, Any]:
        worker = await self._require_session(document_id)
        result = await worker.call(
            "listFileTypes",
            self._type_list_params(regex=regex, limit=limit, cursor=cursor),
        )
        return {
            "documentId": document_id,
            **_summarize_type_page(result.result or {}),
        }

    async def describe_type(self, type_name: str) -> dict[str, Any]:
        temp_worker = WorkerProcess(command=self._worker_command, cwd=self._worker_cwd)
        try:
            result = await temp_worker.call(
                "describeType",
                {"typeName": type_name},
            )
            return result.result or {}
        finally:
            await temp_worker.terminate()

    async def get_objects(
        self,
        document_id: str,
        *,
        handles: list[str],
        projection: str | None = None,
        select: list[str] | None = None,
    ) -> dict[str, Any]:
        worker = await self._require_session(document_id)
        params: dict[str, Any] = {"handles": handles}
        if projection is not None:
            params["projection"] = projection
        if select is not None:
            params["select"] = select
        result = await worker.call("getObjects", params)
        return {
            "documentId": document_id,
            **(result.result or {}),
        }

    async def query_objects(self, document_id: str, query: dict[str, Any]) -> dict[str, Any]:
        worker = await self._require_session(document_id)
        params = dict(query)
        params.pop("documentId", None)
        result = await worker.call("queryObjects", params)
        return {
            "documentId": document_id,
            **(result.result or {}),
        }

    async def close_all(self) -> None:
        async with self._lock:
            items = list(self._sessions.items())
            self._sessions.clear()

        for _, worker in items:
            with contextlib.suppress(Exception):
                await worker.call("closeFile", {})
            await worker.terminate()

    async def _require_session(self, document_id: str) -> WorkerProcess:
        async with self._lock:
            worker = self._sessions.get(document_id)
        if worker is None:
            raise UnknownDocumentError(f"unknown documentId: {document_id}")
        return worker

    async def _remove_session(self, document_id: str) -> WorkerProcess:
        async with self._lock:
            worker = self._sessions.pop(document_id, None)
        if worker is None:
            raise UnknownDocumentError(f"unknown documentId: {document_id}")
        return worker

    @staticmethod
    def _type_list_params(
        *,
        regex: str | None,
        limit: int | None,
        cursor: str | None,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {}
        if regex is not None:
            params["regex"] = regex
        if limit is not None:
            params["limit"] = limit
        if cursor is not None:
            params["cursor"] = cursor
        return params
