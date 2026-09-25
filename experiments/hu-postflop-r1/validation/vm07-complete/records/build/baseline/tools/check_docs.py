"""Check current documentation entrypoints and local Markdown file links.

This deliberately does not validate remote URLs, heading anchors, or historical
experiment documents. Links from current documentation into experiments are
still checked. No third-party Python packages are required.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[1]
DOCUMENT_ROOTS = ("docs", "tools", "examples", "crates")
REQUIRED_FILES = (
    "AGENTS.md",
    "docs/README.md",
    "docs/status.jp.md",
    "docs/product-roadmap.jp.md",
    "docs/architecture.md",
    "docs/app-architecture.md",
    "docs/development.md",
    "docs/validation.jp.md",
    "docs/solver-config-v1.jp.md",
    "docs/multiway-preflop-v1.jp.md",
    "docs/cli-reference.jp.md",
    "docs/user-guide.jp.md",
    "docs/plans/solver-implementation-plan.jp.md",
    "docs/plans/r0-execution-plan.jp.md",
    "docs/plans/task-template.md",
)
MOVED_PATHS = {
    "research/solver-implementation-plan.jp.md": "docs/plans/solver-implementation-plan.jp.md",
    "research/r0-execution-plan.jp.md": "docs/plans/r0-execution-plan.jp.md",
    "research/hu-postflop-validation-plan.jp.md": "docs/validation.jp.md",
}
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
REFERENCE = re.compile(r"^ {0,3}\[[^\]]+\]:\s*(.*)")
SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")
INLINE_CODE = re.compile(r"(`+)(.*?)\1")


def prose_lines(contents: str):
    """Yield original line numbers, omitting fenced code blocks."""
    fence_char = ""
    fence_length = 0
    for number, line in enumerate(contents.splitlines(), 1):
        match = FENCE.match(line)
        if match:
            marker = match.group(1)
            if not fence_char:
                fence_char, fence_length = marker[0], len(marker)
            elif marker[0] == fence_char and len(marker) >= fence_length:
                if not line[match.end():].strip():
                    fence_char = ""
            continue
        if not fence_char:
            yield number, line


def destination(text: str) -> str:
    """Read a Markdown destination before its optional title/closing paren."""
    text = text.lstrip()
    if text.startswith("<"):
        end = text.find(">")
        return text[1:end] if end >= 0 else ""
    depth = 0
    result = []
    escaped = False
    for char in text:
        if escaped:
            result.append(char)
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == "(":
            depth += 1
            result.append(char)
        elif char == ")":
            if depth == 0:
                break
            depth -= 1
            result.append(char)
        elif char.isspace():
            break
        else:
            result.append(char)
    return "".join(result)


def link_destinations(line: str):
    # Inline code examples are not links. Code inside a link label may be
    # removed safely: the closing bracket and destination remain in the line.
    line = INLINE_CODE.sub("", line)
    reference = REFERENCE.match(line)
    if reference:
        yield destination(reference.group(1))
    for match in re.finditer(r"\]\(", line):
        yield destination(line[match.end():])


def local_target(document: Path, target: str, root: Path) -> Path | None:
    if not target or target.startswith(("#", "//")) or SCHEME.match(target):
        return None
    path = unquote(target.split("#", 1)[0].split("?", 1)[0])
    if not path:
        return None
    # A leading slash is repository-relative, as in rendered repository docs.
    return root / path.lstrip("/") if path.startswith("/") else document.parent / path


def documents(root: Path) -> list[Path]:
    paths = set(root.glob("*.md"))
    for name in DOCUMENT_ROOTS:
        paths.update((root / name).rglob("*.md"))
    return sorted(paths)


def check(root: Path = ROOT) -> list[str]:
    errors = []
    for name in REQUIRED_FILES:
        if not (root / name).is_file():
            errors.append(f"{name}: missing required documentation entrypoint")
    for document in documents(root):
        relative = document.relative_to(root).as_posix()
        historical = relative.startswith("docs/research/")
        for number, line in prose_lines(document.read_text(encoding="utf-8-sig")):
            if not historical:
                for old, new in MOVED_PATHS.items():
                    if old in line:
                        errors.append(f"{relative}:{number}: moved path {old}; use {new}")
            for target in link_destinations(line):
                resolved = local_target(document, target, root)
                if resolved is not None and not resolved.exists():
                    errors.append(f"{relative}:{number}: missing local link target: {target}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help="repository to check")
    args = parser.parse_args()
    root = args.root.resolve()
    errors = check(root)
    if errors:
        print("\n".join(errors))
        print(f"Documentation check failed: {len(errors)} problem(s).")
        return 1
    print(f"Documentation check passed: {len(documents(root))} Markdown files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
