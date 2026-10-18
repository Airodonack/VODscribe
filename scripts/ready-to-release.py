"""Checks main is ready to tag as a release, and prints the commands to do it.

    python3 scripts/ready-to-release.py

Run after the version bump is pushed. It doesn't tag anything: tagging
publishes, so that stays a person's decision. Fails (listing every problem)
unless:

  git        the working tree is clean, on main, and the same commit as origin/main
  version    Cargo.toml's version is a release (not 0.4.1-dev: between releases
             main carries the next version with -dev), has a dated CHANGELOG.md
             entry, Cargo.lock agrees, and its tag doesn't exist yet
  CI         the checks and the installer builds passed on this very commit
             (a build replaced by a newer push doesn't count: run it by hand,
             `gh workflow run build.yml`)

Then it lists what can't be checked from here: the review passes, and
whether the changelog entry has what people would notice: every commit
summary since the last release (they're written for people who use the
app), less the ones about tests and CI, to read against it.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402

REPO = "Airodonack/VODscribe"


def run(*args: str) -> str:
    return subprocess.run(args, cwd=release.ROOT, check=True, capture_output=True, text=True).stdout.strip()


def ci_result(workflow: str, sha: str) -> str:
    """The latest run of `workflow` on `sha`: its conclusion, its status if unfinished, or "no run"."""
    runs = json.loads(
        run("gh", "api", f"repos/{REPO}/actions/workflows/{workflow}/runs?head_sha={sha}&per_page=1")
    )["workflow_runs"]
    if not runs:
        return "no run"
    return runs[0]["conclusion"] or runs[0]["status"]


# Summaries of commits that don't change the app for people who use it.
NOT_FOR_USERS = re.compile(r"end-to-end|\be2e\b|\btests?\b|\bCI\b|workflow|dev builds?|commit message|harness", re.I)


def since_release() -> list[str]:
    """The summaries of the commits since the last release tag that may matter to people."""
    last = run("git", "describe", "--tags", "--abbrev=0", "--match", "v*")
    lines = run("git", "log", "--format=%s", f"{last}..HEAD").splitlines()
    return [line for line in lines if not NOT_FOR_USERS.search(line)]


def next_dev(version: str) -> str:
    """The version main carries after `version` is released: 0.4.0 -> 0.4.1-dev."""
    major, minor, patch = (int(n) for n in version.split("-")[0].split(".")[:3])
    return f"{major}.{minor}.{patch + 1}-dev"


def main() -> None:
    problems: list[str] = []
    version = release.cargo_version((release.ROOT / "Cargo.toml").read_text())

    if run("git", "status", "--porcelain", "--untracked-files=no"):
        problems.append("the working tree has uncommitted changes")
    if run("git", "branch", "--show-current") != "main":
        problems.append("not on main")
    run("git", "fetch", "--quiet", "--tags", "origin", "main")
    sha = run("git", "rev-parse", "HEAD")
    if sha != run("git", "rev-parse", "origin/main"):
        problems.append("this commit isn't origin/main: push, or pull")

    if "-" in version:
        problems.append(f"{version} is a development version: bump it to the release's (drop the -dev)")
    body, unreleased = release.changelog_entry(release.CHANGELOG.read_text(), version)
    if unreleased:
        problems.append(f"CHANGELOG.md still marks {version} as unreleased: date its heading")
    elif not body:
        problems.append(f"CHANGELOG.md's entry for {version} is empty")
    lock = (release.ROOT / "Cargo.lock").read_text()
    stale = [n for n, v in re.findall(r'name = "(vodscribe[^"]*)"\nversion = "([^"]+)"', lock) if v != version]
    if stale:
        problems.append(f"Cargo.lock has another version for {', '.join(stale)}: run cargo check")
    if run("git", "tag", "--list", f"v{version}"):
        problems.append(f"v{version} is already tagged: bump the version")

    for workflow, what in [("check.yml", "checks"), ("build.yml", "installer builds")]:
        result = ci_result(workflow, sha)
        if result != "success":
            problems.append(f"the {what} ({workflow}) on {sha[:7]}: {result}")

    if problems:
        sys.exit(f"Not ready to release {version}:\n  " + "\n  ".join(problems))
    print(f"""Ready to release {version} ({sha[:7]}), as far as can be checked from here.

Before tagging, check what can't be:
  - the security, correctness and code quality review passes are done
  - anything only testable by hand was tried in this build
  - CHANGELOG.md's entry has what people would notice of these (newest
    first; a change to something new in this release is part of it, not news):
{chr(10).join("      " + line for line in since_release())}

Then, to publish:
  git tag v{version} && git push origin v{version}

The publish job checks the release once it's up (scripts/verify-release.py).

Once it's out:
  - the website, from the release: Cloudflare builds vodscribe.com from the
    site branch, so its demo shows what people can download, not main
      git push origin v{version}^{{commit}}:refs/heads/site
    (refused if site has fixes of its own since: merge the tag into it)
  - main goes on as the next version, in development, in a commit of its
    own: Cargo.toml (and Cargo.lock, by cargo check) to {next_dev(version)}, and
    an empty "## {next_dev(version).split("-")[0]} (unreleased)" entry atop CHANGELOG.md""")


if __name__ == "__main__":
    if len(sys.argv) != 1:
        sys.exit(__doc__)
    main()
