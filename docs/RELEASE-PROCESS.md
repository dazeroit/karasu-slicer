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

Pre-release versions carry a suffix: `2.0.0-alpha.1`, `2.0.0-beta.1` and `2.0.0-rc.1` are
released for testing (see [Beta releases](#beta-releases)); `2.0.0-dev` is the version of `main`
while it develops 2.0 and is never released. Semantic Versioning orders them
`dev < alpha < beta < rc <` the release.

After every release, the branch it was made from moves at once, by pull request, to the next
version with `-dev`: after `v0.1.0`, `main` becomes `0.2.0-dev`; after `v1.2.0`, `release/1.x`
becomes `1.3.0-dev`. A build from source then never claims to be a version already released.

A version has at most 23 characters, so that the GOO header can hold it; a longer one fails the
build. The commit a build comes from is not part of the version.

Before 1.0, the `0.x` versions are previews released from `main`. They receive no fixes of their
own; a fix ships in the next preview.

## Beta releases

A beta release lets users try a version before it is final. It is released like any other
version (see [Tags and releases](#tags-and-releases)), from a tag with a pre-release suffix, and
published on GitHub as a pre-release, so GitHub's latest release stays the stable one.

| Suffix | Meaning | Tagged on |
|---|---|---|
| `-alpha.N` | features still missing or changing; for early feedback | `main`, for the next major |
| `-beta.N` | features complete, defects expected | `main` for the next major; `release/N.x` for its next minor |
| `-rc.N` | ready unless a defect is found | `release/N.x` (see [Cutting a release series](#cutting-a-release-series)) |

Before 1.0, betas of the next preview are tagged on `main`. The numbers restart for every version
(`2.0.0-beta.1`, `2.0.0-beta.2`, then `2.0.0-rc.1`); a version need not go through every stage.

Betas use the same settings and profile library as the stable version. A beta that changes their
format, which only a major version may do, says so at the top of its release notes. Packages
order pre-releases correctly: the `.deb` writes `2.0.0~beta.1`, which Debian sorts before
`2.0.0`.

Nightly builds are not published (TD-151).

## Tags and releases

- Every release has an annotated tag `vX.Y.Z` (`v1.0.0-rc.1` for a release candidate) on the
  commit that sets that version. Tags of 1.0 and later are on release branches; preview tags are
  on `main`.
- Tags are never moved or deleted. A wrong release, or a draft that fails its trial, is followed by
  a new version.

To release a version:

1. The commit that sets the version has passed CI on both systems and the complete native Windows
   validation ([tools/windows-native](../tools/windows-native/README.md)).
2. The owner creates the annotated tag and pushes it:
   `git tag -a vX.Y.Z -m "karasu-slicer X.Y.Z"`, then `git push origin vX.Y.Z`.
3. The tag starts `.github/workflows/release.yml` ([TD-150](TECHNOLOGY-DECISIONS.md#td-150--release-builds-and-distribution)).
   It checks that the tag matches the version in `Cargo.toml`, builds the packages of
   [tools/packaging](../tools/packaging/README.md) from the tagged commit on GitHub's runners,
   writes `SHA256SUMS`, attests the files' build provenance and opens a draft release with them,
   the download notes and the list of merged pull requests.
4. The owner installs the draft's packages and runs the application on real hardware, on Windows
   and on Linux, then writes what changed at the top of the notes and publishes the draft.
   Previews (`0.x`) and every version with a suffix (alpha, beta, rc) are published as
   pre-releases, so that GitHub's latest release is always a stable one.
5. A pull request moves the branch to the next `-dev` version (see [Versions](#versions)).

GitHub Releases is the only distribution channel for now. The Windows packages are not signed:
Windows warns before running them, and the release notes say so.

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
