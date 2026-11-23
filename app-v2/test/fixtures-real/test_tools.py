"""合成数据测试：只使用占位/合成名称，绝不引用真实语料。"""

import hashlib
import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import export_snapshot
import sync_files
import verify

HERE = Path(__file__).resolve().parent

# 合成表结构，用于构造最小可用索引库
TABLE_DDL = {
    "files": "id INTEGER PRIMARY KEY, name TEXT, path TEXT, archived_path TEXT, size INTEGER",
    "archive_dirs": "id INTEGER PRIMARY KEY, path TEXT, name TEXT",
    "tags": "id INTEGER PRIMARY KEY, name TEXT, color TEXT",
    "file_tags": "file_id INTEGER, tag_id INTEGER",
    "assignments": "id INTEGER PRIMARY KEY, file_id INTEGER, batch_id INTEGER",
    "batches": "id INTEGER PRIMARY KEY, name TEXT, created_at TEXT",
    "migrations": "id INTEGER PRIMARY KEY, name TEXT, created_at TEXT",
    "migration_items": (
        "id INTEGER PRIMARY KEY, migration_id INTEGER, file_id INTEGER, "
        "dst_path TEXT, result TEXT"
    ),
    # 与线上真实结构一致：无 role 列
    "source_roots": "id INTEGER PRIMARY KEY, path TEXT, enabled INTEGER, added_at INTEGER",
    "settings": "key TEXT, value TEXT",
}


def _sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


class FixtureToolsTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)

        self.source = self.root / "fake_source"
        self.archive = self.root / "fake_archive"
        self.subdir = self.archive / "测试目录"
        self.source.mkdir()
        self.subdir.mkdir(parents=True)

        self.fake_files = [
            self.archive / "测试文件A.docx",
            self.archive / "测试文件B.xlsx",
            self.subdir / "测试文件C.pdf",
        ]
        for i, path in enumerate(self.fake_files):
            path.write_text(f"synthetic-content-{i}\n", encoding="utf-8")

        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        (self.root / ".gitignore").write_text("_data/\n", encoding="utf-8")
        self.target = self.root / "_data"

        self.db = self.root / "index.db"
        self._build_db()
        self.snapshot = self.root / "snapshot.json"
        export_snapshot.export_snapshot(str(self.db), str(self.snapshot))

    def tearDown(self):
        self._tmp.cleanup()

    def _build_db(self):
        conn = sqlite3.connect(str(self.db))
        try:
            for table, ddl in TABLE_DDL.items():
                conn.execute(f"CREATE TABLE {table} ({ddl})")

            rows = {
                "files": [
                    (1, self.fake_files[0].name, str(self.source / self.fake_files[0].name),
                     str(self.fake_files[0]), 20),
                    (2, self.fake_files[1].name, str(self.source / self.fake_files[1].name),
                     str(self.fake_files[1]), 20),
                    (3, self.fake_files[2].name, str(self.source / self.fake_files[2].name),
                     str(self.fake_files[2]), 20),
                ],
                "archive_dirs": [
                    (1, str(self.archive), "fake_archive"),
                    (2, str(self.subdir), "测试目录"),
                ],
                "tags": [
                    (1, "测试标签甲", "#112233"),
                    (2, "测试标签乙", "#445566"),
                ],
                "file_tags": [(1, 1), (2, 2), (3, 1)],
                "assignments": [(1, 1, 1), (2, 2, 1), (3, 3, 1)],
                "batches": [(1, "测试批次", "2026-01-01T00:00:00Z")],
                "migrations": [(1, "测试迁移", "2026-01-02T00:00:00Z")],
                "migration_items": [
                    (1, 1, 1, str(self.fake_files[0]), "ok"),
                    (2, 1, 2, str(self.fake_files[1]), "ok"),
                    (3, 1, 3, str(self.fake_files[2]), "ok"),
                ],
                "source_roots": [(1, str(self.source), 1, 0)],
                "settings": [
                    ("archive_root", str(self.archive)),
                    ("appVersion", "synthetic"),
                ],
            }
            for table, table_rows in rows.items():
                placeholders = ",".join("?" for _ in table_rows[0])
                conn.executemany(
                    f"INSERT INTO {table} VALUES ({placeholders})", table_rows
                )
            conn.commit()
        finally:
            conn.close()

    def test_export_all_tables_and_meta(self):
        data = json.loads(self.snapshot.read_text(encoding="utf-8"))
        self.assertIn("meta", data)
        self.assertIn("tables", data)
        for table in TABLE_DDL:
            self.assertIn(table, data["tables"])
            self.assertEqual(
                data["meta"]["rowCounts"][table], len(data["tables"][table])
            )
        self.assertTrue(data["meta"]["capturedAt"])
        self.assertEqual(data["meta"]["sourceRoot"], str(self.source))
        self.assertEqual(data["meta"]["archiveRoot"], str(self.archive))
        self.assertIn("archived_path", data["tables"]["files"][0])

    def test_source_root_from_source_roots_without_role_column(self):
        """线上 source_roots 无 role 列；sourceRoot 只能从该表取。"""
        data = json.loads(self.snapshot.read_text(encoding="utf-8"))
        self.assertEqual(data["meta"]["sourceRoot"], str(self.source))
        self.assertEqual(data["meta"]["archiveRoot"], str(self.archive))

    def test_sync_copies_by_relative_path_and_writes_checksums(self):
        result = sync_files.sync_files(str(self.snapshot), str(self.target))
        self.assertEqual(result["errors"], [])
        self.assertEqual(len(result["copied"]), 3)

        for rel in ["测试文件A.docx", "测试文件B.xlsx", "测试目录/测试文件C.pdf"]:
            copied = self.target / "files" / rel
            self.assertTrue(copied.is_file(), msg=f"缺少复制结果: {rel}")

        manifest = (self.target / "checksums.sha256").read_text(encoding="utf-8")
        for path in self.fake_files:
            rel = path.relative_to(self.archive).as_posix()
            line = next(
                line for line in manifest.splitlines() if line.startswith(f"{rel}  ")
            )
            digest = line.split("  ")[1]
            self.assertEqual(digest, _sha256(path))

    def test_sync_is_idempotent(self):
        sync_files.sync_files(str(self.snapshot), str(self.target))
        copies = sorted((self.target / "files").rglob("*"))
        copies = [p for p in copies if p.is_file()]
        before = {str(p): p.stat().st_mtime_ns for p in copies}

        result = sync_files.sync_files(str(self.snapshot), str(self.target))
        self.assertEqual(result["errors"], [])
        self.assertEqual(result["copied"], [])
        self.assertEqual(len(result["skipped"]), 3)
        after = {str(p): p.stat().st_mtime_ns for p in copies}
        self.assertEqual(before, after)

    def test_sync_reports_missing_source(self):
        missing = self.fake_files[0]
        missing.unlink()
        proc = subprocess.run(
            [sys.executable, str(HERE / "sync_files.py"),
             str(self.snapshot), str(self.target)],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        output = proc.stdout + proc.stderr
        self.assertIn(str(missing), output)
        self.assertIn("缺失", output)

    def test_verify_green_then_tamper(self):
        sync_files.sync_files(str(self.snapshot), str(self.target))
        self.assertEqual(verify.verify(str(self.snapshot), str(self.target)), [])

        proc = subprocess.run(
            [sys.executable, str(HERE / "verify.py"),
             str(self.snapshot), str(self.target)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 0)

        tampered = self.target / "files" / "测试文件A.docx"
        tampered.write_text("被篡改的内容\n", encoding="utf-8")
        errors = verify.verify(str(self.snapshot), str(self.target))
        self.assertTrue(errors)
        self.assertTrue(any("sha256" in err.lower() for err in errors))

        proc = subprocess.run(
            [sys.executable, str(HERE / "verify.py"),
             str(self.snapshot), str(self.target)],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("sha256", (proc.stdout + proc.stderr).lower())


if __name__ == "__main__":
    unittest.main()
