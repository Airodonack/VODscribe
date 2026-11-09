"""Checks main is ready to tag, and prints the commands to do it.

    python3 scripts/ready-to-release.py

Run on main once its version commit is pushed. Main freezes when the release
round starts: from then until the tag only fixes land on it, and features
wait on their own branches. The version commit goes on main's tip, and a fix
after it gets a fresh version commit there. The tag's run is the one full
lane: it builds every system, runs every end-to-end shard and publishes. It
doesn't tag anything: tagging publishes, so that stays a person's decision.
Fails (listing every problem) unless:

  git        the working tree is clean, on main, and the same commit as
             origin/main
  version    Cargo.toml's version is a release (not 0.4.1+dev: between releases
             main carries the next version with +dev; the app, its installers
             and the site all take theirs from it), has a dated CHANGELOG.md
             entry, Cargo.lock agrees, and its tag doesn't exist yet
  commit     HEAD is the "Version X.Y.Z" commit and its body carries the entry
             (the tag goes on it and nothing else)
  changelog  the entry's new behaviour comes first and its fixes last, as
             sub-bullets under one closing "- Fixes:" bullet, and every
             bullet opens with a headline of eight words or fewer, with no
             colon or parenthesis (the releases page shows it plain)
  e2e        the end-to-end tests' ledgers (tests/e2e: durations.json,
             flaky.json, budgets.json) name only scenarios that exist
  CI         the checks (check.yml) passed on this very commit

Then it lists what can't be checked from here: the release round on the
test machines, the review passes, and whether the changelog entry has what
people would notice: every commit summary since the last release (they're
written for people who use the app), less the ones about tests and CI, to
read against it. Either way it names the scenarios tests/e2e/durations.json
has no time for yet, for the pre-release round to refresh.

Once the tag's run has published, the site branch moves to the tag, main
goes on as the next version, and the features that waited land.
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
    """The summaries of the commits since the last release tag that may matter
    to people (a merge is no change of its own)."""
    last = run("git", "describe", "--tags", "--abbrev=0", "--match", "v*")
    lines = run("git", "log", "--no-merges", "--format=%s", f"{last}..HEAD").splitlines()
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
    # Each bullet opens with a short headline, which the releases page shows
    # plain and the rest muted.
    for line in lines:
        bullet = re.match(r"\s*- (.*)", line)
        if bullet and bullet[1] != "Fixes:" and (why := headline_problem(bullet[1])):
            problems.append(f'the bullet "{" ".join(bullet[1].split()[:5])}..." {why}')
    return problems


HEADLINE_WORDS = 8


def headline(text: str) -> str:
    """A bullet's first sentence, split as the releases page splits it
    (apps/site/src/lib/changelog.ts `sentences`): at the first ". " followed
    by a capital letter outside a `code` run, or at the end."""
    for m in re.finditer(r"\. (?=[A-Z])", text):
        before = text[: m.start() + 1]
        if before.count("`") % 2 == 0:
            return before
    return text


def headline_problem(text: str) -> str:
    """Why a bullet's headline is too long or not plain, or "" when it's fine."""
    first = headline(text)
    words = len(first.split())
    if words > HEADLINE_WORDS:
        return f"opens with {words} words before its first full stop (at most {HEADLINE_WORDS})"
    if ":" in first or "(" in first:
        return "has a colon or a parenthesis in its headline"
    return ""


def commit_problems(subject: str, message: str, version: str, entry: str) -> list[str]:
    """What's wrong with the commit to tag: it is the version commit, and its
    body carries the changelog entry (GitHub's view of the tag shows the
    commit, so the message is the release notes). v0.6.0 and v0.7.0 were
    tagged on e2e fixes pushed after the version commit; a fix after it
    gets a fresh version commit on main."""
    problems = []
    if subject.strip() != f"Version {version}":
        problems.append(f'HEAD is "{subject.strip()}", not "Version {version}": the tag goes on the version commit, last before the tag')
        return problems
    body = {line.strip() for line in message.splitlines()}
    missing = [line.strip() for line in entry.splitlines() if line.strip().startswith("- ") and line.strip() not in body]
    if missing:
        problems.append(f"the version commit's body lacks {len(missing)} line(s) of CHANGELOG.md's entry, first: {missing[0][:60]}")
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


def git_problems(root: Path) -> list[str]:
    """What's wrong with the checkout at `root` as the release: clean, on
    main, and main's tip on origin (a stale checkout would tag an old
    commit)."""

    def git(*args: str) -> subprocess.CompletedProcess:
        return subprocess.run(("git", *args), cwd=root, check=False, capture_output=True, text=True)

    problems = []
    if git("status", "--porcelain", "--untracked-files=no").stdout.strip():
        problems.append("the working tree has uncommitted changes")
    branch = git("branch", "--show-current").stdout.strip()
    if branch != "main":
        problems.append(f"HEAD is on {branch or 'no branch'}, not main: the version commit goes on main's tip")
        return problems
    git("fetch", "--quiet", "--tags", "origin", "main")
    head = git("rev-parse", "HEAD").stdout.strip()
    pushed = git("rev-parse", "--verify", "--quiet", "refs/remotes/origin/main").stdout.strip()
    if not pushed:
        problems.append("main isn't on origin: push it")
    elif pushed != head:
        problems.append("this commit isn't origin/main: push, or pull")
    return problems


def next_dev(version: str) -> str:
    """The version main carries after `version` is released: 0.4.0 -> 0.4.1+dev."""
    major, minor, patch = (int(n) for n in version.split("+")[0].split(".")[:3])
    return f"{major}.{minor}.{patch + 1}+dev"


def main() -> None:
    problems: list[str] = []
    version = release.cargo_version((release.ROOT / "Cargo.toml").read_text())

    problems += git_problems(release.ROOT)
    sha = run("git", "rev-parse", "HEAD")

    if "+" in version:
        problems.append(f"{version} is a development version: bump it to the release's (drop the +dev)")
    # A development version's entry is the release's (0.4.1+dev's is 0.4.1's),
    # so its other problems are listed too.
    body, unreleased = release.changelog_entry(release.CHANGELOG.read_text(), version.split("+")[0])
    if unreleased:
        problems.append(f"CHANGELOG.md still marks {version} as unreleased: date its heading")
    elif not body:
        problems.append(f"CHANGELOG.md's entry for {version} is empty")
    problems += [f"CHANGELOG.md's entry for {version}: {p}" for p in entry_problems(body)]
    problems += commit_problems(run("git", "log", "-1", "--format=%s"), run("git", "log", "-1", "--format=%b"), version.split("+")[0], body)
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

    # The installer builds and the end-to-end tests run on the tag: that run
    # is the release's one full lane, so none is asked for before it.
    result = ci_result("check.yml", sha)
    if result != "success":
        problems.append(f"the checks (check.yml) on {sha[:7]}: {result}")

    if problems:
        sys.exit(f"Not ready to release {version}:\n  " + "\n  ".join(problems) + (f"\nAlso: {timing}" if timing else ""))
    print(f"""Ready to release {version} ({sha[:7]}), as far as can be checked from here.

Before tagging, check what can't be:
  - the release round on the test machines passed on this release's code
  - the security, correctness, races and code quality review passes are done
  - anything only testable by hand was tried in this build{chr(10) + "  - " + timing if timing else ""}
  - CHANGELOG.md's entry has what people would notice of these (newest
    first; a change to something new in this release is part of it, not news):
{chr(10).join("      " + line for line in since_release())}

Then, to publish (a signed, annotated tag: the repo signs tags with the
same key as commits; a bare `git tag` would make an unsigned one):
  git tag -s v{version} -m "Version {version}" {sha[:12]} && git tag -v v{version} && git push origin v{version}
(the tag names the version commit, so it lands there from any checkout)

The tag's run is the full lane: every build, every system's end-to-end
shards, then the publish job, which checks the release once it's up
(scripts/verify-release.py). A one-off failure: re-run the failed jobs. A
failure that needs a commit: on main, still frozen, revert the version
commit (git revert --no-edit HEAD), land the fix, then make a fresh version
commit. The owner deletes the unpublished tag
(git push origin :refs/tags/v{version}) and the new version commit is
tagged v{version} again.

Once it's out:
  - the website, from the release: Cloudflare builds vodscribe.com from the
    site branch, so its demo shows what people can download, not main
      git push origin v{version}^{{commit}}:refs/heads/site
    refused when site has commits of its own (fixes made there, often on
    main too by now): merge the tag into it; the diff is empty when the
    merge's tree is the release's, else site has a change the release lacks
      git fetch origin site && git switch --detach origin/site
      git merge --no-edit v{version} && git diff --stat v{version}
      git push origin HEAD:refs/heads/site
  - main goes on as the next version, in development, in a commit of its
    own: Cargo.toml (and Cargo.lock, by cargo check) to {next_dev(version)}, and
    an empty "## {next_dev(version).split("+")[0]} (unreleased)" entry atop CHANGELOG.md;
    python3 scripts/release.py --check keeps the two in step
  - main's freeze ends: the features that waited land, each rebased onto
    main""")


if __name__ == "__main__":
    if len(sys.argv) != 1:
        sys.exit(__doc__)
    main()
