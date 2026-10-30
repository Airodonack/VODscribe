"""Checks main is ready to tag as a release, and prints the commands to do it.

    python3 scripts/ready-to-release.py

Run after the version bump is pushed. It doesn't tag anything: tagging
publishes, so that stays a person's decision. Fails (listing every problem)
unless:

  git        the working tree is clean, on main, and the same commit as origin/main
  version    Cargo.toml's version is a release (not 0.4.1-dev: between releases
             main carries the next version with -dev; the app, its installers
             and the site all take theirs from it), has a dated CHANGELOG.md
             entry, Cargo.lock agrees, and its tag doesn't exist yet
  changelog  the entry's new behaviour comes first and its fixes last, as
             sub-bullets under one closing "- Fixes:" bullet
  e2e        the end-to-end tests' ledgers (tests/e2e: durations.json,
             flaky.json, budgets.json) name only scenarios that exist
  CI         the checks and the installer builds passed on this very commit
             (a build replaced by a newer push doesn't count: run it by hand,
             `gh workflow run build.yml`)

Then it lists what can't be checked from here: the review passes, and
whether the changelog entry has what people would notice: every commit
summary since the last release (they're written for people who use the
app), less the ones about tests and CI, to read against it. Either way it
names the scenarios tests/e2e/durations.json has no time for yet, for the
pre-release round to refresh.
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


def entry_problems(body: str) -> list[str]:
    """What's out of place in a changelog entry: the fixes go last, as
    sub-bullets under one "- Fixes:" bullet, after the new behaviour."""
    top = [line for line in body.splitlines() if line.startswith("- ")]
    problems = []
    if top.count("- Fixes:") > 1:
        problems.append('it has more than one "- Fixes:" bullet')
    elif "- Fixes:" in top and top[-1] != "- Fixes:":
        problems.append('"- Fixes:" is not its last bullet: new behaviour goes above it')
    lines = body.splitlines()
    fixes = lines.index("- Fixes:") if "- Fixes:" in lines else len(lines)
    if any(line.startswith("  - ") for line in lines[:fixes]):
        problems.append('it has sub-bullets above "- Fixes:": a fix goes under it')
    # A merge's or a rebase's conflict left in, which the releases page would show.
    if any(re.match(r"\s*(<{7}|\|{7}|={7}|>{7})( |$)", line) for line in lines):
        problems.append("it has a merge conflict's marker left in")
    return problems


def scenarios(e2e: Path) -> set[str]:
    """The end-to-end scenarios in `e2e` (tests/e2e)."""
    return {p.stem for p in (e2e / "scenarios").glob("*.mjs")}


def ledger_problems(e2e: Path) -> list[str]:
    """What the end-to-end tests' ledgers in `e2e` (tests/e2e) say of
    scenarios that aren't there."""
    names = scenarios(e2e)
    durations = json.loads((e2e / "durations.json").read_text())
    flaky = json.loads((e2e / "flaky.json").read_text())
    budgeted = {
        name for platform in json.loads((e2e / "budgets.json").read_text()).values()
        if isinstance(platform, dict) for name in platform.get("scenarios", {})
    }
    problems = []
    for file, listed in [("durations.json", set(durations)), ("flaky.json", set(flaky)), ("budgets.json", budgeted)]:
        if gone := sorted(listed - names):
            problems.append(f"tests/e2e/{file} names scenarios that aren't there: {', '.join(gone)}")
    return problems


def unmeasured(e2e: Path) -> list[str]:
    """The scenarios durations.json has no time for, which CI's shards then
    deal as the median: new ones, or ones skipped where it was refreshed
    (those needing the network); a round that runs them refreshes it."""
    return sorted(scenarios(e2e) - set(json.loads((e2e / "durations.json").read_text())))


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
    # A development version's entry is the release's (0.4.1-dev's is 0.4.1's),
    # so its other problems are listed too.
    body, unreleased = release.changelog_entry(release.CHANGELOG.read_text(), version.split("-")[0])
    if unreleased:
        problems.append(f"CHANGELOG.md still marks {version} as unreleased: date its heading")
    elif not body:
        problems.append(f"CHANGELOG.md's entry for {version} is empty")
    problems += [f"CHANGELOG.md's entry for {version}: {p}" for p in entry_problems(body)]
    e2e = release.ROOT / "tests" / "e2e"
    problems += ledger_problems(e2e)
    # Not a reason to wait, but worth doing with the pre-release round.
    untimed = unmeasured(e2e)
    timing = (
        f"tests/e2e/durations.json has no time for {', '.join(untimed)}: refresh it from a round"
        " that runs them (node tests/e2e/durations.mjs <out dir>...)"
        if untimed
        else ""
    )
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
        sys.exit(f"Not ready to release {version}:\n  " + "\n  ".join(problems) + (f"\nAlso: {timing}" if timing else ""))
    print(f"""Ready to release {version} ({sha[:7]}), as far as can be checked from here.

Before tagging, check what can't be:
  - the security, correctness, races and code quality review passes are done
  - anything only testable by hand was tried in this build{chr(10) + "  - " + timing if timing else ""}
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
