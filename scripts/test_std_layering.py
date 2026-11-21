"""R4.1 unit tests: the std layering check resolves imports, keeps core
self-contained, and reports violations with file context."""

import tempfile
import unittest
from pathlib import Path

import std_layering


class LayeringTests(unittest.TestCase):
    def build(self, files: dict[str, str]) -> Path:
        root = Path(tempfile.mkdtemp())
        for name, source in files.items():
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source, encoding="utf-8", newline="\n")
        return root

    def test_missing_import_target_is_reported(self):
        root = self.build({"core/broken.ae": 'use "does_not_exist.ae";\n'})
        violations = std_layering.check(root)
        self.assertEqual(len(violations), 1)
        self.assertIn("does not resolve", violations[0])

    def test_core_reaching_up_is_reported(self):
        root = self.build({
            "core/lower.ae": "pub fn value() -> i64 { return 1; }\n",
            "io/hosted.ae": "pub fn hosted() -> i64 { return 2; }\n",
            "core/climber.ae": 'use "../io/hosted.ae";\n',
        })
        violations = std_layering.check(root)
        self.assertEqual(len(violations), 1)
        self.assertIn("core imports hosted module io/hosted.ae", violations[0])

    def test_core_importing_core_and_hosted_importing_core_pass(self):
        root = self.build({
            "core/lower.ae": "pub fn value() -> i64 { return 1; }\n",
            "core/upper.ae": 'use "lower.ae";\n',
            "io/hosted.ae": 'use "../core/lower.ae";\n',
        })
        self.assertEqual(std_layering.check(root), [])

    def test_real_std_tree_holds(self):
        violations = std_layering.check()
        self.assertEqual(
            violations, [],
            f"the real std tree violates layering: {violations[:8]}",
        )


if __name__ == "__main__":
    unittest.main()
