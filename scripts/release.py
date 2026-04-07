#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import pathlib
import re
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
CHANGELOG = ROOT / "CHANGELOG.md"
RELEASE_NOTES = ROOT / ".release-notes.md"
INTERNAL_CRATES = [
    "syft-cli",
    "syft-core",
    "syft-git",
    "syft-objects",
    "syft-semantic",
    "syft-store",
    "syft-types",
    "syft-validate",
]
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")


def run(*args: str) -> str:
    completed = subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True)
    return completed.stdout.strip()


def maybe_run(*args: str) -> tuple[int, str]:
    completed = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    return completed.returncode, completed.stdout.strip()


def current_version(text: str) -> str:
    match = re.search(r'(?ms)^\[workspace\.package\]\n.*?^version = "([^"]+)"', text)
    if not match:
        raise SystemExit("could not find workspace version in Cargo.toml")
    return match.group(1)


def bump_version(version: str, level: str) -> str:
    major, minor, patch = [int(part) for part in version.split(".")]
    if level == "major":
        return f"{major + 1}.0.0"
    if level == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def sync_versions(text: str, version: str) -> str:
    updated = re.sub(
        r'(?ms)(^\[workspace\.package\]\n.*?^version = )"[^"]+"',
        rf'\1"{version}"',
        text,
        count=1,
    )
    for crate in INTERNAL_CRATES:
        updated = re.sub(
            rf'(^{crate} = \{{ version = )"[^"]+"(, path = "crates/{crate}" \}}$)',
            rf'\1"{version}"\2',
            updated,
            flags=re.MULTILINE,
        )
    return updated


def latest_tag() -> str | None:
    code, output = maybe_run("git", "describe", "--tags", "--abbrev=0")
    return output if code == 0 and output else None


def tag_exists(tag: str) -> bool:
    code, _ = maybe_run("git", "rev-parse", "-q", "--verify", f"refs/tags/{tag}")
    return code == 0


def changelog_entries(since_tag: str | None) -> list[str]:
    args = ["git", "log", "--pretty=format:%s"]
    if since_tag:
        args.append(f"{since_tag}..HEAD")
    output = run(*args)
    entries = []
    for line in output.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("chore(release):"):
            continue
        entries.append(line)
    return entries


def update_changelog(version: str, notes: str, existing_text: str) -> str:
    heading = f"## v{version} ({dt.date.today().isoformat()})"
    if heading in existing_text:
        return existing_text

    body = notes.strip() if notes.strip() else "- Release updates"
    section = f"{heading}\n\n{body}\n\n"
    marker = "<!-- version list -->"
    if marker in existing_text:
        return existing_text.replace(marker, f"{marker}\n\n{section}", 1)
    return f"# CHANGELOG\n\n{section}{existing_text}"


def extract_release_notes(version: str, text: str) -> str:
    pattern = re.compile(
        rf"(?ms)^## v{re.escape(version)} \([^)]+\)\n\n(.*?)(?=^## |\Z)"
    )
    match = pattern.search(text)
    if match:
        return match.group(1).strip() + "\n"
    return f"Release v{version}\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version")
    parser.add_argument("--bump", choices=["patch", "minor", "major"], default="patch")
    parser.add_argument("--notes", default="")
    args = parser.parse_args()

    cargo_text = CARGO_TOML.read_text()
    current = current_version(cargo_text)
    version = args.version or bump_version(current, args.bump)

    if not SEMVER_RE.match(version):
        raise SystemExit(f"invalid semantic version: {version}")

    tag = f"v{version}"
    existing = tag_exists(tag)

    if not existing:
        CARGO_TOML.write_text(sync_versions(cargo_text, version))

        previous_tag = latest_tag()
        lines = [f"- {entry}" for entry in changelog_entries(previous_tag)]
        notes = args.notes.strip()
        if notes:
            lines.insert(0, notes)
        changelog_text = CHANGELOG.read_text() if CHANGELOG.exists() else "# CHANGELOG\n\n<!-- version list -->\n"
        CHANGELOG.write_text(update_changelog(version, "\n".join(lines), changelog_text))

    release_notes_source = CHANGELOG.read_text() if CHANGELOG.exists() else ""
    RELEASE_NOTES.write_text(extract_release_notes(version, release_notes_source))

    print(f"version={version}")
    print(f"tag={tag}")
    print(f"existing_tag={'true' if existing else 'false'}")
    print(f"release_notes_file={RELEASE_NOTES.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
