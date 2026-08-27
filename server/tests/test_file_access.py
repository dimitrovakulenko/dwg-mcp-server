from __future__ import annotations

import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from dwg_mcp_server.file_access import (
    ensure_within_roots,
    file_uri_to_path,
    normalize_local_path,
    resolve_root_relative_path,
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


class FileAccessTests(unittest.TestCase):
    def test_file_uri_to_path_accepts_local_file_uris(self) -> None:
        house_plan = (repo_root() / "testData" / "house_plan.dwg").resolve()
        self.assertEqual(file_uri_to_path(house_plan.as_uri()), house_plan)

    def test_normalize_local_path_rejects_relative_paths(self) -> None:
        with self.assertRaisesRegex(ValueError, "absolute local path"):
            normalize_local_path("testData/house_plan.dwg")

    def test_file_uri_to_path_rejects_non_file_scheme(self) -> None:
        with self.assertRaisesRegex(ValueError, "file://"):
            file_uri_to_path("https://example.com/house_plan.dwg")

    def test_ensure_within_roots_rejects_paths_outside_roots(self) -> None:
        house_plan = (repo_root() / "testData" / "house_plan.dwg").resolve()
        with self.assertRaisesRegex(ValueError, "outside the client roots"):
            ensure_within_roots(house_plan, [(repo_root() / "server").resolve()])

    def test_resolve_root_relative_path_accepts_dwg_under_root(self) -> None:
        resolved = resolve_root_relative_path(
            repo_root() / "testData",
            "house_plan.dwg",
        )
        self.assertEqual(resolved, (repo_root() / "testData" / "house_plan.dwg").resolve())

    def test_resolve_root_relative_path_accepts_dxf_under_root(self) -> None:
        with TemporaryDirectory() as directory:
            drawing = Path(directory) / "drawing.dxf"
            drawing.touch()
            self.assertEqual(
                resolve_root_relative_path(Path(directory), "drawing.dxf"), drawing.resolve()
            )

    def test_resolve_root_relative_path_rejects_absolute_paths(self) -> None:
        with self.assertRaisesRegex(ValueError, "must be relative"):
            resolve_root_relative_path(repo_root() / "testData", str(repo_root()))

    def test_resolve_root_relative_path_rejects_traversal(self) -> None:
        with self.assertRaisesRegex(ValueError, "escapes"):
            resolve_root_relative_path(repo_root() / "testData", "../README.md")

    def test_resolve_root_relative_path_rejects_non_cad_files(self) -> None:
        with self.assertRaisesRegex(ValueError, ".dxf"):
            resolve_root_relative_path(repo_root(), "README.md")
