"""按归档相对路径复制真实文件副本，并写出 sha256 清单（幂等、只读源）。"""

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path


def _to_posix(path):
    return str(path).replace("\\", "/")


def _sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _relative_to_archive(path, archive_root):
    """返回相对归档根的 POSIX 路径；不在归档根下时返回 None。"""
    normalized = _to_posix(path)
    root = _to_posix(archive_root).rstrip("/")
    if not root:
        return None
    if normalized.lower() == root.lower():
        return ""
    prefix = root + "/"
    if normalized.lower().startswith(prefix.lower()):
        return normalized[len(prefix):]
    return None


def sync_files(snapshot_path, target_dir):
    data = json.loads(Path(snapshot_path).read_text(encoding="utf-8"))
    meta = data.get("meta", {})
    tables = data.get("tables", {})
    archive_root = meta.get("archiveRoot")

    result = {"copied": [], "skipped": [], "missing": [], "errors": [], "entries": []}
    if not archive_root:
        result["errors"].append("snapshot.meta.archiveRoot 为空，无法确定归档根")
        return result

    items = tables.get("migration_items", [])
    files_by_id = {row.get("id"): row for row in tables.get("files", [])}

    target = Path(target_dir)
    files_dir = target / "files"
    files_dir.mkdir(parents=True, exist_ok=True)

    seen = set()
    for item in items:
        if str(item.get("result", "")).lower() != "ok":
            continue

        source = item.get("dst_path") or item.get("dst") or item.get("destination_path")
        if not source:
            fallback = files_by_id.get(item.get("file_id")) or {}
            source = (
                fallback.get("archived_path")
                or fallback.get("dst_path")
                or fallback.get("path")
            )
        if not source:
            result["errors"].append(
                f"migration_item #{item.get('id')} 缺少 dst_path 且无法回退到 files.archived_path"
            )
            continue

        rel = _relative_to_archive(source, archive_root)
        if not rel:
            rel = _to_posix(source).lstrip("/")
        if rel in seen:
            continue
        seen.add(rel)

        src = Path(source)
        if not src.is_file():
            result["missing"].append(str(source))
            result["errors"].append(f"源文件缺失: {source}")
            continue

        try:
            source_hash = _sha256_file(src)
            dest = files_dir / rel
            if dest.is_file() and _sha256_file(dest) == source_hash:
                result["skipped"].append(rel)
            else:
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src, dest)
                result["copied"].append(rel)
            result["entries"].append((rel, source_hash))
        except OSError as exc:
            result["errors"].append(f"复制失败 {source}: {exc}")

    lines = [f"{rel}  {digest}" for rel, digest in result["entries"]]
    manifest = "\n".join(lines) + ("\n" if lines else "")
    (target / "checksums.sha256").write_text(manifest, encoding="utf-8")
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description="复制真实文件副本并生成 sha256 清单")
    parser.add_argument("snapshot", help="快照 JSON 路径")
    parser.add_argument("target", help="目标目录（语料 _data 目录）")
    args = parser.parse_args(argv)
    try:
        result = sync_files(args.snapshot, args.target)
    except Exception as exc:
        print(f"同步失败: {exc}", file=sys.stderr)
        return 1

    for error in result["errors"]:
        print(f"错误: {error}", file=sys.stderr)
    print(
        f"复制 {len(result['copied'])} 份，跳过(已一致) {len(result['skipped'])} 份，"
        f"清单 {len(result['entries'])} 条"
    )
    return 1 if result["errors"] else 0


if __name__ == "__main__":
    sys.exit(main())
