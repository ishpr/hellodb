#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REQUIRED_METADATA_KEYS = {
    "id",
    "title",
    "summary",
    "category",
    "owner",
    "last_updated",
    "entrypoints",
}
SECRET_PATTERNS = [
    re.compile(r"sk-[A-Za-z0-9]{20,}"),
    re.compile(r"xoxb-[A-Za-z0-9-]{20,}"),
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    re.compile(r"(?:CLOUDFLARE|OPENROUTER|SUPABASE)_.*?=.*", re.IGNORECASE),
]


def fail(msg: str) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(1)


def validate_metadata(meta_path: Path) -> None:
    try:
        data = json.loads(meta_path.read_text())
    except Exception as exc:
        fail(f"{meta_path}: invalid JSON ({exc})")

    missing = REQUIRED_METADATA_KEYS - set(data.keys())
    if missing:
        fail(f"{meta_path}: missing keys {sorted(missing)}")
    if not isinstance(data["entrypoints"], list) or not data["entrypoints"]:
        fail(f"{meta_path}: entrypoints must be a non-empty array")


def validate_readme(readme_path: Path) -> None:
    txt = readme_path.read_text()
    required_headers = ["## What It Does", "##"]
    if not any(h in txt for h in required_headers):
        fail(f"{readme_path}: missing required section headings")
    for patt in SECRET_PATTERNS:
        if patt.search(txt):
            fail(f"{readme_path}: potential secret pattern matched `{patt.pattern}`")


def validate_collection(base: Path, allow_template: bool = False) -> None:
    if not base.exists():
        return
    for child in sorted(base.iterdir()):
        if not child.is_dir():
            continue
        if allow_template and child.name == "_template":
            continue

        readme = child / "README.md"
        meta = child / "metadata.json"
        if not readme.exists():
            fail(f"{child}: missing README.md")
        if not meta.exists():
            fail(f"{child}: missing metadata.json")
        validate_readme(readme)
        validate_metadata(meta)


def main() -> None:
    validate_collection(ROOT / "recipes", allow_template=True)
    validate_collection(ROOT / "integrations")
    print("ok: contribution metadata/docs validation passed")


if __name__ == "__main__":
    main()
