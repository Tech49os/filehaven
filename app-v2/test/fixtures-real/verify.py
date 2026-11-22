"""校验快照、文件副本、sha256 清单与 git 忽略状态。"""

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path


def _sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _is_hex64(text):
    return len(text) == 64 and all(c in "0123456789abcdefABCDEF" for c in text)


def _parse_manifest(path):
    entries = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.rstrip("\r")
        if not line.strip():
            continue
        parts = line.split("  ")
        if len(parts) < 2:
            continue
        first, second = parts[0], parts[1]
        if _is_hex64(first):
            digest, rel = first, second
        else:
            rel, digest = first, second
        entries[rel] = digest
    return entries


def _is_git_ignored(sample):
    sample = Path(sample)
    try:
        probe = sample.parent if sample.parent.is_dir() else sample
        top = subprocess.run(
            ["git", "-C", str(probe), "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
        )
        if top.returncode != 0:
            return False
        repo = Path(top.stdout.strip())
        rel = sample.resolve().relative_to(repo.resolve()).as_posix()
        check = subprocess.run(
            ["git", "-C", str(repo), "check-ignore", "-q", rel],
            capture_output=True,
        )
        return check.returncode == 0
    except (OSError, ValueError):
        return False


def verify(snapshot_path, target_dir):
    """返回错误列表；空列表表示全部通过。"""
    errors = []
    snapshot = json.loads(Path(snapshot_path).read_text(encoding="utf-8"))
    meta = snapshot.get("meta", {})
    tables = snapshot.get("tables", {})
    counts = meta.get("rowCounts", {})

    for name, rows in tables.items():
        if name not in counts:
            errors.append(f"meta.rowCounts 缺少表 {name} 的统计")
        elif counts[name] != len(rows):
            errors.append(
                f"表 {name} 行数不一致: meta={counts[name]}, 实际={len(rows)}"
            )
    for name in counts:
        if name not in tables:
            errors.append(f"meta.rowCounts 记录了不存在的表 {name}")

    target = Path(target_dir)
    files_dir = target / "files"
    ok_count = sum(
        1
        for item in tables.get("migration_items", [])
        if str(item.get("result", "")).lower() == "ok"
    )
    actual = [p for p in files_dir.rglob("*") if p.is_file()] if files_dir.is_dir() else []
    if len(actual) != ok_count:
        errors.append(f"文件数不一致: 实际={len(actual)}, 期望(ok)={ok_count}")

    manifest_path = target / "checksums.sha256"
    if not manifest_path.is_file():
        errors.append("缺少 checksums.sha256")
    else:
        for rel, digest in _parse_manifest(manifest_path).items():
            copied = files_dir / rel
            if not copied.is_file():
                errors.append(f"清单中的文件不存在: {rel}")
                continue
            actual_hash = _sha256_file(copied)
            if actual_hash.lower() != digest.lower():
                errors.append(
                    f"sha256 不一致: {rel} (清单={digest}, 实际={actual_hash})"
                )

    sample = actual[0] if actual else (files_dir if files_dir.exists() else target)
    if not _is_git_ignored(sample):
        errors.append(f"_data 未被 git 忽略: {sample}")

    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description="校验快照与真实文件副本的一致性")
    parser.add_argument("snapshot", help="快照 JSON 路径")
    parser.add_argument("target", help="语料 _data 目录")
    args = parser.parse_args(argv)
    try:
        errors = verify(args.snapshot, args.target)
    except Exception as exc:
        print(f"校验异常: {exc}", file=sys.stderr)
        return 1

    for error in errors:
        print(f"失败: {error}", file=sys.stderr)
    if errors:
        print(f"校验未通过，共 {len(errors)} 项失败", file=sys.stderr)
        return 1
    print("校验通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
