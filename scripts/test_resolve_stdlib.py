#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("resolve-stdlib.py")
SPEC = importlib.util.spec_from_file_location("resolve_stdlib", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
resolver = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = resolver
SPEC.loader.exec_module(resolver)


class ResolverTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def source(self, name: str, header: str = "依存: なし") -> None:
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"%{{ fixture\n   {header}\n}}%\n1\n", encoding="utf-8")

    def error_code(self, targets: tuple[str, ...] = ()) -> str:
        with self.assertRaises(resolver.ResolverError) as caught:
            resolver.resolve(resolver.load_sources(self.root), targets)
        return caught.exception.code

    def test_string_json_jsonl_chain(self) -> None:
        self.source("string.vaak")
        self.source("codec/json_utf8.vaak", "依存: stdlib/string.vaak")
        self.source(
            "codec/jsonl_utf8.vaak",
            "依存: stdlib/string.vaak, stdlib/codec/json_utf8.vaak",
        )

        order = resolver.resolve(
            resolver.load_sources(self.root), ("codec/jsonl_utf8.vaak",)
        )

        self.assertEqual(
            order,
            ["string.vaak", "codec/json_utf8.vaak", "codec/jsonl_utf8.vaak"],
        )

    def test_old_bullet_header_is_supported(self) -> None:
        self.source("array/i64/search_binary.vaak")
        self.source("array/i64/sort/merge.vaak")
        self.source(
            "array/i64/compress.vaak",
            "前置き依存:\n   - array/i64/search_binary.vaak\n   - array/i64/sort/merge.vaak",
        )

        order = resolver.resolve(
            resolver.load_sources(self.root), ("array/i64/compress.vaak",)
        )

        self.assertEqual(
            order,
            [
                "array/i64/search_binary.vaak",
                "array/i64/sort/merge.vaak",
                "array/i64/compress.vaak",
            ],
        )

    def test_ready_sources_use_utf8_byte_tie_break(self) -> None:
        self.source("z.vaak")
        self.source("a.vaak")
        self.source("app.vaak", "依存: z.vaak, a.vaak")

        order = resolver.resolve(resolver.load_sources(self.root), ("app.vaak",))

        self.assertEqual(order, ["a.vaak", "z.vaak", "app.vaak"])

    def test_non_ascii_names_also_use_utf8_byte_tie_break(self) -> None:
        self.source("あ.vaak")
        self.source("β.vaak")
        self.source("é.vaak")
        self.source("app.vaak", "依存: あ.vaak, β.vaak, é.vaak")

        order = resolver.resolve(resolver.load_sources(self.root), ("app.vaak",))

        self.assertEqual(order, ["é.vaak", "β.vaak", "あ.vaak", "app.vaak"])

    def test_non_canonical_spelling_is_rejected_before_normalization(self) -> None:
        self.source("a/b.vaak")
        for spelling in ["a//b.vaak", "a/./b.vaak", "./a/b.vaak"]:
            with self.subTest(spelling=spelling):
                self.source("app.vaak", f"依存: {spelling}")
                self.assertEqual(self.error_code(("app.vaak",)), "E103")

    def test_missing_dependency_is_an_error(self) -> None:
        self.source("app.vaak", "依存: missing.vaak")

        self.assertEqual(self.error_code(("app.vaak",)), "E201")

    def test_cycle_is_an_error(self) -> None:
        self.source("a.vaak", "依存: b.vaak")
        self.source("b.vaak", "依存: a.vaak")

        self.assertEqual(self.error_code(("a.vaak",)), "E202")

    def test_duplicate_dependency_is_an_error(self) -> None:
        self.source("base.vaak")
        self.source("app.vaak", "依存: base.vaak, stdlib/base.vaak")

        self.assertEqual(self.error_code(("app.vaak",)), "E102")

    def test_duplicate_target_is_an_error(self) -> None:
        self.source("app.vaak")

        self.assertEqual(self.error_code(("app.vaak", "stdlib/app.vaak")), "E203")

    def test_concat_has_one_blank_separator_and_final_newline(self) -> None:
        self.source("base.vaak")
        self.source("app.vaak", "依存: base.vaak")
        order = resolver.resolve(resolver.load_sources(self.root), ("app.vaak",))

        combined = resolver.concatenate(self.root, order)

        self.assertTrue(combined.endswith("\n"))
        self.assertIn("\n\n%{ fixture", combined)

    def test_cli_missing_and_cycle_use_status_2_and_stable_stderr_code(self) -> None:
        self.source("missing.vaak", "依存: absent.vaak")
        missing = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--root",
                str(self.root),
                "missing.vaak",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(missing.returncode, 2)
        self.assertTrue(missing.stderr.startswith("[E201] "), missing.stderr)
        self.assertEqual(missing.stdout, "")

        self.source("a.vaak", "依存: b.vaak")
        self.source("b.vaak", "依存: a.vaak")
        cycle = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.root), "a.vaak"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(cycle.returncode, 2)
        self.assertTrue(cycle.stderr.startswith("[E202] "), cycle.stderr)
        self.assertEqual(cycle.stdout, "")


if __name__ == "__main__":
    unittest.main()
