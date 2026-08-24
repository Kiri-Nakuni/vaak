#!/usr/bin/env python3
"""Vaak stdlib source の明示依存を決定的に並べる。"""

from __future__ import annotations

import argparse
import heapq
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Iterable, Sequence


DEPENDENCY_LINE = re.compile(r"^\s*(前置き依存|依存)\s*:\s*(.*?)\s*$")
BULLET_LINE = re.compile(r"^\s*-\s*(.*?)\s*$")


@dataclass(frozen=True)
class ResolverError(Exception):
    code: str
    detail: str

    def __str__(self) -> str:
        return f"[{self.code}] {self.detail}"


@dataclass(frozen=True)
class Source:
    name: str
    path: Path
    dependencies: tuple[str, ...]


def _sort_key(name: str) -> bytes:
    """locale に依存しない UTF-8 byte 順。"""

    return name.encode("utf-8")


def _canonical_name(raw: str, *, owner: str) -> str:
    value = raw.strip()
    if value.startswith("stdlib/"):
        value = value[len("stdlib/") :]
    path = PurePosixPath(value)
    if (
        not value
        or value == "なし"
        or value != path.as_posix()
        or path.is_absolute()
        or "\\" in value
        or any(part in ("", ".", "..") for part in path.parts)
        or path.suffix != ".vaak"
    ):
        raise ResolverError("E103", f"{owner}: 不正な依存source名 {raw!r}")
    return path.as_posix()


def _leading_comment_lines(text: str) -> list[str]:
    """先頭の `% ... %` / `%{ ... }%` だけから本文を取り出す。"""

    lines = text.splitlines()
    result: list[str] = []
    index = 0
    in_block = False
    saw_comment = False
    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if in_block:
            if "}%" in line:
                before, _separator, _after = line.partition("}%")
                result.append(before)
                in_block = False
            else:
                result.append(line)
            index += 1
            continue
        if not stripped:
            if saw_comment:
                result.append("")
            index += 1
            continue
        if stripped.startswith("%{"):
            saw_comment = True
            after = line[line.index("%{") + 2 :]
            if "}%" in after:
                before, _separator, _tail = after.partition("}%")
                result.append(before)
            else:
                result.append(after)
                in_block = True
            index += 1
            continue
        if stripped.startswith("%") and stripped.endswith("%") and len(stripped) >= 2:
            saw_comment = True
            result.append(stripped[1:-1])
            index += 1
            continue
        break
    return result


def parse_dependencies(text: str, *, owner: str) -> tuple[str, ...]:
    lines = _leading_comment_lines(text)
    declarations = 0
    raw_dependencies: list[str] = []
    index = 0
    while index < len(lines):
        match = DEPENDENCY_LINE.match(lines[index])
        if match is None:
            index += 1
            continue
        declarations += 1
        if declarations > 1:
            raise ResolverError("E101", f"{owner}: 依存欄が二つ以上ある")
        kind, rest = match.groups()
        if rest and rest != "なし":
            raw_dependencies.extend(part.strip() for part in rest.split(","))
        if kind == "前置き依存":
            index += 1
            while index < len(lines):
                bullet = BULLET_LINE.match(lines[index])
                if bullet is None:
                    break
                raw_dependencies.append(bullet.group(1))
                index += 1
            continue
        index += 1

    dependencies: list[str] = []
    seen: set[str] = set()
    for raw in raw_dependencies:
        dependency = _canonical_name(raw, owner=owner)
        if dependency in seen:
            raise ResolverError(
                "E102", f"{owner}: 依存source {dependency!r} が重複している"
            )
        seen.add(dependency)
        dependencies.append(dependency)
    return tuple(dependencies)


def load_sources(root: Path) -> dict[str, Source]:
    if not root.is_dir():
        raise ResolverError("E001", f"stdlib root がdirectoryでない: {root}")
    sources: dict[str, Source] = {}
    paths = sorted(root.rglob("*.vaak"), key=lambda path: _sort_key(path.relative_to(root).as_posix()))
    for path in paths:
        if not path.is_file():
            continue
        name = path.relative_to(root).as_posix()
        if name in sources:
            raise ResolverError("E104", f"source名 {name!r} が重複している")
        text = path.read_text(encoding="utf-8")
        sources[name] = Source(name, path, parse_dependencies(text, owner=name))
    return sources


def _requested_closure(sources: dict[str, Source], targets: Sequence[str]) -> set[str]:
    if not targets:
        requested = sorted(sources, key=_sort_key)
    else:
        requested = []
        seen_targets: set[str] = set()
        for raw in targets:
            target = _canonical_name(raw, owner="command line")
            if target in seen_targets:
                raise ResolverError("E203", f"target {target!r} が重複している")
            seen_targets.add(target)
            requested.append(target)

    closure: set[str] = set()
    pending = list(reversed(sorted(requested, key=_sort_key)))
    while pending:
        name = pending.pop()
        source = sources.get(name)
        if source is None:
            code = "E200" if name in requested else "E201"
            raise ResolverError(code, f"source {name!r} が見つからない")
        if name in closure:
            continue
        closure.add(name)
        for dependency in sorted(source.dependencies, key=_sort_key, reverse=True):
            if dependency not in sources:
                raise ResolverError(
                    "E201", f"{name}: 依存source {dependency!r} が見つからない"
                )
            pending.append(dependency)
    return closure


def resolve(sources: dict[str, Source], targets: Sequence[str] = ()) -> list[str]:
    closure = _requested_closure(sources, targets)
    indegree = {name: 0 for name in closure}
    dependents = {name: [] for name in closure}
    for name in closure:
        for dependency in sources[name].dependencies:
            if dependency in closure:
                indegree[name] += 1
                dependents[dependency].append(name)

    ready: list[tuple[bytes, str]] = [
        (_sort_key(name), name) for name, degree in indegree.items() if degree == 0
    ]
    heapq.heapify(ready)
    order: list[str] = []
    while ready:
        _key, name = heapq.heappop(ready)
        order.append(name)
        for dependent in sorted(dependents[name], key=_sort_key):
            indegree[dependent] -= 1
            if indegree[dependent] == 0:
                heapq.heappush(ready, (_sort_key(dependent), dependent))

    if len(order) != len(closure):
        members = sorted(
            (name for name, degree in indegree.items() if degree > 0), key=_sort_key
        )
        raise ResolverError("E202", "依存cycle: " + " -> ".join(members))
    return order


def concatenate(root: Path, order: Iterable[str]) -> str:
    chunks: list[str] = []
    for name in order:
        text = (root / name).read_text(encoding="utf-8")
        chunks.append(text.rstrip("\n") + "\n")
    return "\n".join(chunks)


def _argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("targets", nargs="*", help="stdlib/ からの相対source名")
    parser.add_argument("--root", type=Path, default=Path("stdlib"))
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--json", action="store_true", help="順序をJSON arrayで出す")
    output.add_argument("--concat", action="store_true", help="順序どおりのsource本文を出す")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _argument_parser().parse_args(argv)
    try:
        sources = load_sources(args.root)
        order = resolve(sources, args.targets)
        if args.concat:
            sys.stdout.write(concatenate(args.root, order))
        elif args.json:
            json.dump(order, sys.stdout, ensure_ascii=False, separators=(",", ":"))
            sys.stdout.write("\n")
        else:
            for name in order:
                print(name)
        return 0
    except (OSError, UnicodeError) as error:
        print(f"[E002] sourceを読めない: {error}", file=sys.stderr)
        return 2
    except ResolverError as error:
        print(error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
