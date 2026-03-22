from __future__ import annotations

import os
import unittest
from pathlib import Path
from unittest.mock import patch

from dwg_mcp_server.file_access import (
    configured_access_folders,
    ensure_within_roots,
    file_uri_to_path,
    format_access_folders,
    normalize_local_path,
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

    def test_configured_access_folders_supports_semicolon_separated_paths(self) -> None:
        with patch.dict(
            os.environ,
            {
                "DWG_MCP_HOST_FOLDERS": f"{repo_root() / 'testData'}; {repo_root() / 'server'}"
            },
            clear=False,
        ):
            self.assertEqual(
                configured_access_folders(),
                [(repo_root() / "testData").resolve(), (repo_root() / "server").resolve()],
            )

    def test_format_access_folders_is_human_readable(self) -> None:
        rendered = format_access_folders(
            [(repo_root() / "testData").resolve(), (repo_root() / "server").resolve()]
        )
        self.assertIn(str((repo_root() / "testData").resolve()), rendered)
        self.assertIn(str((repo_root() / "server").resolve()), rendered)
