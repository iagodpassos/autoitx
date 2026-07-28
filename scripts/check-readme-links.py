#!/usr/bin/env python3
"""Check that README.md's links survive the renderer crates.io uses.

The README is read in three places that do not agree with each other, and
0.1.0 shipped with ten dead links because only one of them was ever checked.

crates.io does not serve the file from the repository. It renders the markdown
itself and rewrites relative links against a base it derives on its own, which
for this workspace is wrong twice over: `autoitx/Cargo.toml` points at
`../README.md`, and the rewrite prepends the package directory anyway, so
`autoitx/examples` shipped as

    https://github.com/iagodpassos/autoitx/blob/HEAD/autoitx/autoitx/examples

with the directory doubled. It also rewrites every relative link to `/blob/`,
so a link to a *directory* cannot come out right even with the correct base.
Both were invisible on GitHub, where the same links resolve fine.

Hence the rules below: no relative links, and every link into this repository
has to name something that is really here.

Heading anchors have their own trap. Both crates.io (comrak) and GitHub id
their headings `user-content-<slug>`; only GitHub ships the JavaScript that
also makes the bare `#<slug>` work. So `#user-content-license` works in both
places and `#license` works in one.

    ./scripts/check-readme-links.py             # offline, what CI runs
    ./scripts/check-readme-links.py --network   # also HTTP-check outbound URLs
"""

from __future__ import annotations

import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO = "https://github.com/iagodpassos/autoitx"
BRANCH = "main"
ROOT = Path(__file__).resolve().parent.parent

DEFINITION = re.compile(r"^\[([^\]]+)\]:[ \t]*(\S+)[ \t]*$", re.M)
INLINE = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
REFERENCE = re.compile(r"\]\[([^\]]+)\]")
HEADING = re.compile(r"^#{1,6}[ \t]+(.+?)[ \t]*#*$", re.M)
REPO_PATH = re.compile(rf"^{re.escape(REPO)}/(blob|tree)/([^/]+)/(.+)$")


def without_code(markdown: str) -> str:
    """Blank out fenced code blocks, keeping line numbers intact.

    A `](` inside a code sample is not a link, and the Quickstart is full of
    Rust that could plausibly contain one.
    """
    lines, in_fence = [], False
    for line in markdown.splitlines():
        is_fence = re.match(r"[ \t]*(```|~~~)", line) is not None
        if is_fence:
            in_fence = not in_fence
        lines.append("" if in_fence or is_fence else line)
    return "\n".join(lines)


def slug(heading: str) -> str:
    """The anchor id GitHub and comrak both derive from a heading."""
    text = re.sub(r"[^\w\s-]", "", heading.strip().lower(), flags=re.UNICODE)
    return re.sub(r"\s+", "-", text)


def validate(url: str, anchors: set[str]) -> list[str]:
    """Everything wrong with one link target, ignoring where it lives."""
    if url.startswith("#"):
        name = url[1:]
        if not name.startswith("user-content-"):
            return [
                f"{url} works on GitHub but not on crates.io — "
                f"write it as #user-content-{name}"
            ]
        if name not in anchors:
            return [f"{url} matches no heading in this file"]
        return []

    if not url.startswith(("http://", "https://")):
        return [
            f"relative link {url!r} — crates.io rewrites these against the "
            f"wrong base; use the full {REPO}/... URL"
        ]

    match = REPO_PATH.match(url)
    if not match:
        return []

    kind, ref, path = match.groups()
    target = ROOT / path
    problems = []
    if ref != BRANCH:
        problems.append(f"{url} points at {ref!r}, not {BRANCH!r}")
    if not target.exists():
        problems.append(f"{url} — {path} is not in the repository")
    elif kind != (wanted := "tree" if target.is_dir() else "blob"):
        problems.append(f"{url} — {path} needs /{wanted}/, not /{kind}/")
    return problems


def check(markdown: str) -> list[str]:
    body = without_code(markdown)
    lines = body.splitlines()
    problems: list[str] = []

    anchors = {f"user-content-{slug(h)}" for h in HEADING.findall(body)}
    definitions = {label.lower(): url for label, url in DEFINITION.findall(body)}

    for number, line in enumerate(lines, 1):
        # A reference with no definition renders as literal text — brackets
        # and all — rather than failing, so only reading the page catches it.
        for label in REFERENCE.findall(line):
            if label.lower() not in definitions:
                problems.append(f"line {number}: [{label}] is used but never defined")

        definition = DEFINITION.match(line)
        targets = [definition.group(2)] if definition else INLINE.findall(line)
        for url in targets:
            problems += [f"line {number}: {p}" for p in validate(url, anchors)]

    # Counting `[label]` rather than parsing: a definition contributes one
    # occurrence, so a used one appears at least twice. Shortcut references
    # like `[AutoItX]` are caught by this where a stricter parser would need
    # to know the three reference syntaxes apart.
    for label in definitions:
        if len(re.findall(rf"\[{re.escape(label)}\]", body, re.I)) < 2:
            problems.append(f"[{label}] is defined but never used")

    return problems


def fetch(url: str) -> tuple[int | None, str, str | None]:
    """Status and final URL, falling back to GET when a server dislikes HEAD.

    Both headers are load-bearing, and neither is obvious. crates.io serves a
    single-page app: without `Accept: text/html` it answers 404 for a crate
    that demonstrably exists, and with no `User-Agent` at all it answers 403.
    The GET fallback is for servers that simply refuse HEAD.
    """
    headers = {"User-Agent": "Mozilla/5.0", "Accept": "text/html,*/*"}
    for method in ("HEAD", "GET"):
        request = urllib.request.Request(url, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                return response.status, response.geturl(), None
        except urllib.error.HTTPError as error:
            if method == "GET":
                return None, url, f"HTTP {error.code}"
        except Exception as error:  # noqa: BLE001 — a lint must not die on DNS
            return None, url, f"{type(error).__name__}: {error}"
    return None, url, "unreachable"


def check_network(markdown: str) -> list[str]:
    """Fetch every outbound URL.

    Kept out of CI: it depends on other people's servers staying up, and a
    lint that fails for reasons the commit cannot control gets ignored.

    Redirects are reported but not failed. Some are canonical — docs.rs always
    redirects `/crate` to `/crate/latest/crate` — and a rule that failed on
    them would be muted within a week. They still get printed, because that is
    how the MSRV badge was caught: it answered 200, but only after redirecting
    to a stub, the Rust blog having changed its URL scheme.
    """
    body = without_code(markdown)
    found = INLINE.findall(body) + [url for _, url in DEFINITION.findall(body)]
    urls = sorted({u for u in found if u.startswith(("http://", "https://"))})

    problems, moved = [], []
    for url in urls:
        status, final, error = fetch(url)
        if error:
            problems.append(f"{url}\n    {error}")
            continue
        print(f"  {status}  {url}", flush=True)
        if final.rstrip("/") != url.rstrip("/"):
            moved.append(f"{url}\n      -> {final}")

    if moved:
        print("\n  Redirects — not failures, but check that each is intended:", flush=True)
        for entry in moved:
            print(f"    {entry}", flush=True)
    return problems


def main() -> int:
    paths = [a for a in sys.argv[1:] if not a.startswith("-")]
    readme = Path(paths[0]) if paths else ROOT / "README.md"
    markdown = readme.read_text(encoding="utf-8")

    problems = check(markdown)
    if "--network" in sys.argv:
        print("Checking outbound URLs...")
        problems += check_network(markdown)

    if problems:
        print(f"\n{readme.name}: {len(problems)} problem(s)\n", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1

    print(f"{readme.name}: links OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
