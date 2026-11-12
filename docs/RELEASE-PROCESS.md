# Branches, versions and releases

How changes reach `main`, how versions are numbered, and how a release series is cut and
maintained. The decision and its alternatives are recorded in
[TD-149](TECHNOLOGY-DECISIONS.md#td-149--branches-versions-and-releases).

## Branches

| Branch | Holds | Receives |
|---|---|---|
| `main` | the development of the next major version | pull requests from topic branches |
| `release/N.x` | the maintained series N (for example `release/1.x`) | fixes and safe additions, by pull request |
| topic branches | one change each (`feat/…`, `fix/…`, `docs/…`) | the author's commits |

- `main` always passes the [checks](../README.md#checks-before-a-change). It is never rewritten
  or deleted.
- A topic branch lives only as long as its pull request and is deleted when it is merged.
- A release branch is created when its major version is frozen, not before (see
  [Cutting a release series](#cutting-a-release-series)). Until 1.0 there is none.

## Pull requests

Every change enters `main` or a release branch through a pull request, the owner's included.

- The checks named **Linux** and **Windows** (`.github/workflows/ci.yml`) and **CLA**
  ([CONTRIBUTING.md](../CONTRIBUTING.md)) must pass. CI runs the checks of the README on Linux and
  on native Windows.
- The branch must be up to date with its target before it is merged.
- Pull requests are squash-merged: each one becomes a single commit, which keeps the history
  linear and lets a fix be carried to a release branch as one commit. The pull request's title
  becomes the commit's title and its description the commit's body, so both follow the rules for
  commit messages in [AGENTS.md](../AGENTS.md#repository-and-git-safety): English, imperative
  title, no attribution trailers.
- The owner may bypass these rules in an emergency, such as a broken `main` that blocks every
  pull request; the commit then says why.

## Versions

karasu-slicer follows [Semantic Versioning 2.0.0](https://semver.org/). The version is set once,
in `[workspace.package]` of the root `Cargo.toml`; the crates, the packages, the installer and the
files karasu-slicer writes take it from there.

For an application, a version is judged by what users have saved or automated:

- **Major** (`2.0.0`): files written by the new version that the previous series cannot open, or
  a migration of the user's files or library that cannot be undone; an incompatible change to the
  command line (commands, options, output, exit codes); a printer or an output format removed.
- **Minor** (`1.3.0`): additions that change none of the above, such as printers and resins in
  the catalog or a new option with the old behaviour as its default.
- **Patch** (`1.3.1`): fixes only.

Pre-release versions carry a suffix: `1.0.0-rc.1` for a release candidate, `2.0.0-dev` on `main`
while it develops the next major.

Before 1.0, the `0.x` versions are previews released from `main`. They receive no fixes of their
own; a fix ships in the next preview.

## Tags and releases

- Every release has an annotated tag `vX.Y.Z` (`v1.0.0-rc.1` for a release candidate) on the
  commit that sets that version. Tags of 1.0 and later are on release branches; preview tags are
  on `main`.
- Tags are never moved or deleted. A wrong release is followed by a new one.
- The release on GitHub carries the packages built from the tagged commit
  ([tools/packaging](../tools/packaging/README.md)) and the release notes.

Before a tag is created, the tagged commit has passed the checks on Linux, CI on both systems, the
complete native Windows validation ([tools/windows-native](../tools/windows-native/README.md))
and a run of the packaged application on real hardware.

## Cutting a release series

When the features of version N.0 are complete:

1. Create `release/N.x` from `main`.
2. On `release/N.x`, set the version to `N.0.0-rc.1`, tag it and publish the release candidate.
3. On `main`, set the version to `(N+1).0.0-dev`.
4. Fix what the release candidates reveal: on `main` first, then carried to `release/N.x`
   (see [Fixes](#fixes)). Each further candidate raises the number: `rc.2`, `rc.3`.
5. When a candidate needs no more fixes, set the version to `N.0.0` on `release/N.x`, tag it and
   publish it.

## On a release branch

A release branch receives, by pull request:

- fixes, released as patch versions;
- additions that change no file format, command line or default behaviour, released as minor
  versions: printers and resins in the catalog, translations, small improvements with no risk to
  existing projects.

It does not receive new features that need a file-format or command-line change, broad
refactorings, or dependency updates other than those that fix a defect or a vulnerability.

Each release from the branch raises its version in a pull request of its own, then the merged
commit is tagged.

## Fixes

A fix goes into `main` first, then into the release branches that need it:

1. Merge the fix into `main` by pull request.
2. On a topic branch from `release/N.x`, apply it with `git cherry-pick -x <commit>`; the `-x`
   records the original commit in the message.
3. Open a pull request into `release/N.x` whose title starts with `[N.x]`.

When the code on `main` has changed too much for the fix to apply, write the fix on the release
branch directly and say in its pull request why it is not a cherry-pick. A fix needed only on the
release branch, because `main` no longer has the defect, says so too.

## Supported series

- The latest series receives every fix.
- The previous series receives fixes for critical defects and vulnerabilities for three months
  after the first release of the next major version, then none.
- [SECURITY.md](../SECURITY.md) lists the series that are supported.

A major version every six to twelve months keeps a release branch close enough to `main` for
fixes to apply.

## Repository rules on GitHub

Rulesets enforce the above; the owner can bypass them.

- `main` and `release/*`: no deletion, no force push, linear history, changes only by pull request,
  squash merge only, the checks **Linux**, **Windows** and **CLA** passing on an up-to-date
  branch.
- Tags `v*`: created only by the owner, never updated or deleted.
- Merged topic branches are deleted automatically.
