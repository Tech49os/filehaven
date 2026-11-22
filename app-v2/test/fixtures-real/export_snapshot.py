"""只读导出索引库快照（表结构动态读取，不硬编码列）。"""

import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

_ROOT_ALIASES = {
    "sourceRoot": {
        "sourceroot", "source_root", "source", "src", "srcroot", "sourcepath",
    },
    "archiveRoot": {
        "archiveroot", "archive_root", "archive", "dst", "dstroot",
        "destination", "destinationroot",
    },
}


def _quote_ident(name):
    return '"' + str(name).replace('"', '""') + '"'


def _jsonable(value):
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return str(value)


def _readonly_uri(db_path):
    """用 file URI 以 mode=ro 打开，确保绝不写入真实索引库。"""
    return Path(db_path).resolve().as_uri() + "?mode=ro"


def _is_enabled(value):
    """enabled 可能是 1 / true / '1'；缺省视为启用。"""
    if value is None:
        return True
    if isinstance(value, str):
        return value.strip().lower() in ("1", "true", "yes", "on")
    return bool(value)


def _extract_roots(tables):
    roots = {"sourceRoot": None, "archiveRoot": None}

    settings = tables.get("settings", [])
    if settings:
        columns = list(settings[0].keys())
        if len(columns) >= 2:
            key_col, value_col = columns[0], columns[1]
            for row in settings:
                key = str(row.get(key_col, "")).strip().lower()
                key = key.replace("-", "_").replace(" ", "")
                for target, aliases in _ROOT_ALIASES.items():
                    if key in aliases and roots[target] is None:
                        roots[target] = row.get(value_col)

    source_roots = tables.get("source_roots", [])
    if source_roots:
        columns = list(source_roots[0].keys())
        role_col = next(
            (c for c in columns if c.lower() in ("role", "kind", "type", "scope")),
            None,
        )
        path_col = next(
            (c for c in columns if c.lower() in ("path", "root", "value", "directory", "dir")),
            None,
        )
        if role_col and path_col:
            for row in source_roots:
                role = str(row.get(role_col, "")).strip().lower()
                if role in ("source", "src") and roots["sourceRoot"] is None:
                    roots["sourceRoot"] = row.get(path_col)
                elif role in ("archive", "archiveroot", "dst", "destination") and roots["archiveRoot"] is None:
                    roots["archiveRoot"] = row.get(path_col)
        elif path_col and roots["sourceRoot"] is None:
            # 线上结构 id/path/enabled/added_at 无 role 列：取首个启用的非空 path
            enabled_col = next(
                (c for c in columns if c.lower() in ("enabled", "active", "is_enabled")),
                None,
            )
            for row in source_roots:
                if enabled_col is not None and not _is_enabled(row.get(enabled_col)):
                    continue
                if row.get(path_col):
                    roots["sourceRoot"] = row.get(path_col)
                    break

    return roots


def export_snapshot(db_path, output_path):
    """返回并写出结构化快照 JSON。"""
    conn = sqlite3.connect(_readonly_uri(db_path), uri=True)
    try:
        names = [
            row[0]
            for row in conn.execute(
                "SELECT name FROM sqlite_master "
                "WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
            )
        ]
        tables = {}
        for name in names:
            columns = [
                row[1]
                for row in conn.execute(f"PRAGMA table_info({_quote_ident(name)})")
            ]
            rows = conn.execute(f"SELECT * FROM {_quote_ident(name)}").fetchall()
            tables[name] = [
                {col: _jsonable(val) for col, val in zip(columns, row)} for row in rows
            ]
    finally:
        conn.close()

    roots = _extract_roots(tables)
    snapshot = {
        "meta": {
            "capturedAt": datetime.now(timezone.utc).isoformat(),
            "rowCounts": {name: len(rows) for name, rows in tables.items()},
            "sourceRoot": roots["sourceRoot"],
            "archiveRoot": roots["archiveRoot"],
        },
        "tables": tables,
    }

    out = Path(output_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as fh:
        json.dump(snapshot, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    return snapshot


def main(argv=None):
    parser = argparse.ArgumentParser(description="只读导出索引库快照 JSON")
    parser.add_argument("index_db", help="索引库路径（只读打开）")
    parser.add_argument("output", help="快照 JSON 输出路径")
    args = parser.parse_args(argv)
    try:
        snapshot = export_snapshot(args.index_db, args.output)
    except Exception as exc:
        print(f"导出失败: {exc}", file=sys.stderr)
        return 1
    print(f"已导出 {len(snapshot['tables'])} 张表 -> {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
