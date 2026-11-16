from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path

from dwg_mcp_server import feedback
from dwg_mcp_server.app import DwgMcpApplication


class ClassifyTests(unittest.TestCase):
    def test_load_failure_phrases(self) -> None:
        for message in (
            "it failed to open my dwg",
            "Cannot open this file, says invalid dwg",
            "this is a 2018 format drawing and it won't open",
        ):
            with self.subTest(message=message):
                self.assertEqual(feedback.classify(message), "file-load-failure")

    def test_write_request(self) -> None:
        self.assertEqual(feedback.classify("I need to edit the layer and save it"), "write-request")

    def test_missing_data(self) -> None:
        self.assertEqual(feedback.classify("hatch boundaries are missing from the objects"), "missing-data")

    def test_rendering_and_performance(self) -> None:
        self.assertEqual(feedback.classify("the png render looks wrong"), "rendering")
        self.assertEqual(feedback.classify("opening is so slow it hangs"), "performance")

    def test_praise_wins_over_other(self) -> None:
        self.assertEqual(feedback.classify("thanks, this works great"), "praise")

    def test_unmatched_is_other(self) -> None:
        self.assertEqual(feedback.classify("hello there"), "other")

    def test_frustration_detection(self) -> None:
        self.assertTrue(feedback.is_frustrated("this is useless and doesn't work"))
        self.assertFalse(feedback.is_frustrated("could you expose block attributes?"))


class BuildResponseTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tempdir = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("DWG_MCP_FEEDBACK_LOG")
        self.log_path = Path(self._tempdir.name) / "nested" / "feedback.jsonl"
        os.environ["DWG_MCP_FEEDBACK_LOG"] = str(self.log_path)

    def tearDown(self) -> None:
        if self._previous is None:
            os.environ.pop("DWG_MCP_FEEDBACK_LOG", None)
        else:
            os.environ["DWG_MCP_FEEDBACK_LOG"] = self._previous
        self._tempdir.cleanup()

    def build(self, message: str, **kwargs: object) -> dict:
        return feedback.build_response(message, server_version="test", **kwargs)  # type: ignore[arg-type]

    def test_report_is_written_to_disk(self) -> None:
        result = self.build("it failed to parse my drawing")
        self.assertTrue(result["recorded"])
        self.assertEqual(result["logPath"], str(self.log_path))

        lines = self.log_path.read_text(encoding="utf-8").strip().splitlines()
        self.assertEqual(len(lines), 1)
        entry = json.loads(lines[0])
        self.assertEqual(entry["category"], "file-load-failure")
        self.assertEqual(entry["message"], "it failed to parse my drawing")
        self.assertEqual(entry["serverVersion"], "test")
        self.assertIn("timestamp", entry)

    def test_reports_append(self) -> None:
        self.build("cannot open file")
        self.build("thanks, works great")
        lines = self.log_path.read_text(encoding="utf-8").strip().splitlines()
        self.assertEqual(len(lines), 2)

    def test_problem_report_offers_both_paths(self) -> None:
        result = self.build("your parser fails to open this dwg")
        steps = " ".join(result["nextSteps"])
        self.assertIn(feedback.ISSUES_URL, steps)
        self.assertIn(feedback.DRAWING_MCP_URL, steps)
        self.assertEqual(result["alternativeServerUrl"], feedback.DRAWING_MCP_URL)

    def test_praise_does_not_advertise(self) -> None:
        result = self.build("thank you, this is great")
        steps = " ".join(result["nextSteps"])
        self.assertIn(feedback.ISSUES_URL, steps)
        self.assertNotIn(feedback.DRAWING_MCP_URL, steps)
        self.assertNotIn("alternativeServerUrl", result)

    def test_feature_request_does_not_advertise(self) -> None:
        result = self.build("please add support for 3d solids, would be nice")
        self.assertEqual(result["category"], "feature-request")
        self.assertNotIn("alternativeServerUrl", result)

    def test_frustration_is_acknowledged(self) -> None:
        result = self.build("this is garbage, it cannot open any of my files")
        self.assertIn("Sorry it wasted your time.", result["note"])
        entry = json.loads(self.log_path.read_text(encoding="utf-8").strip())
        self.assertTrue(entry["frustrated"])

    def test_explicit_category_overrides_classifier(self) -> None:
        result = self.build("hello", category="rendering")
        self.assertEqual(result["category"], "rendering")

    def test_invalid_category_rejected(self) -> None:
        with self.assertRaises(ValueError):
            self.build("hello", category="nonsense")

    def test_empty_message_rejected(self) -> None:
        with self.assertRaises(ValueError):
            self.build("   ")

    def test_long_message_is_truncated(self) -> None:
        self.build("x" * (feedback.MAX_MESSAGE_CHARS + 500))
        entry = json.loads(self.log_path.read_text(encoding="utf-8").strip())
        self.assertEqual(len(entry["message"]), feedback.MAX_MESSAGE_CHARS)

    def test_context_is_recorded(self) -> None:
        self.build("missing entities", context="dwg.query_objects AcDbHatch")
        entry = json.loads(self.log_path.read_text(encoding="utf-8").strip())
        self.assertEqual(entry["context"], "dwg.query_objects AcDbHatch")

    def test_unwritable_log_still_returns_guidance(self) -> None:
        os.environ["DWG_MCP_FEEDBACK_LOG"] = str(
            Path(self._tempdir.name) / "blocked-file" / "feedback.jsonl"
        )
        (Path(self._tempdir.name) / "blocked-file").write_text("not a directory", encoding="utf-8")
        result = self.build("cannot open file")
        self.assertFalse(result["recorded"])
        self.assertNotIn("logPath", result)
        self.assertIn(feedback.DRAWING_MCP_URL, " ".join(result["nextSteps"]))


class ApplicationFeedbackToolTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self._tempdir = tempfile.TemporaryDirectory()
        self._previous = os.environ.get("DWG_MCP_FEEDBACK_LOG")
        os.environ["DWG_MCP_FEEDBACK_LOG"] = str(Path(self._tempdir.name) / "feedback.jsonl")
        self.app = DwgMcpApplication()

    def tearDown(self) -> None:
        if self._previous is None:
            os.environ.pop("DWG_MCP_FEEDBACK_LOG", None)
        else:
            os.environ["DWG_MCP_FEEDBACK_LOG"] = self._previous
        self._tempdir.cleanup()

    async def test_tool_is_listed_with_schema(self) -> None:
        tool = next(
            item for item in self.app.tool_definitions() if item.name == "dwg.report_feedback"
        )
        self.assertEqual(tool.inputSchema["required"], ["message"])
        self.assertEqual(
            tool.inputSchema["properties"]["category"]["enum"], list(feedback.CATEGORIES)
        )

    async def test_tool_call_records_and_responds(self) -> None:
        result = await self.app.call_tool(
            "dwg.report_feedback",
            {"message": "it won't open my 2018 dwg", "context": "dwg.open_file"},
        )
        self.assertEqual(result["category"], "file-load-failure")
        self.assertTrue(result["recorded"])
        self.assertIn(feedback.DRAWING_MCP_URL, " ".join(result["nextSteps"]))


if __name__ == "__main__":
    unittest.main()
