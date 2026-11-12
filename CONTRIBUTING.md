# Contributing

Thank you for considering a contribution to karasu-slicer.

## Before you start

- Read [AGENTS.md](AGENTS.md): it holds the project's permanent rules (English everywhere,
  native Rust geometry, no `unsafe`, clean-room development, reviewed dependencies, tests with
  every change).
- Run the checks listed in the [README](README.md#checks-before-a-change) before you send a
  change.

## Reporting problems and asking for features

Open an issue with one of the forms: **Bug report** asks for the version, the system, the steps
and a diagnostic report (in karasu-slicer: Windows > Log > Save a diagnostic report); **Feature
request** asks what you are trying to do and what karasu-slicer should do. One problem or request
per issue. Vulnerabilities are reported privately instead ([SECURITY.md](SECURITY.md)).

## Branches and pull requests

Branches, versions and releases follow [docs/RELEASE-PROCESS.md](docs/RELEASE-PROCESS.md). In
short:

- Base your work on `main`, in a branch of your fork, and open a pull request into `main`. A fix
  for a released series goes into `main` first; the maintainer carries it to the release branch.
- Keep one change per pull request. Its title is an English sentence in the imperative ("Fix
  the raft outline on rotated models"); its description says what changes and why. They become
  the commit's message when the pull request is squash-merged.
- The checks named **Linux** and **Windows** (CI) and **CLA** must pass, and the branch must be
  up to date with `main`.

## Licence of contributions

karasu-slicer is licensed under the GNU Affero General Public License, version 3 or any later
version ([LICENSE](LICENSE)). The name and the logo are trademarks and are not licensed with it
([TRADEMARKS.md](TRADEMARKS.md)).

Every contribution is accepted only once its author has agreed to the project's Contributor
Licence Agreement ([CLA.md](CLA.md)). The agreement leaves you the copyright of your work. It
gives the project's owner the right to distribute it under the AGPL and also under other
licences, such as a commercial licence, so that the project can be offered to those who cannot
use the AGPL.

To agree, post this comment in your pull request, exactly:

> I have read the karasu-slicer Contributor Licence Agreement (CLA.md, version 1) and I agree to
> its terms for all my contributions to karasu-slicer.

Every author of the pull request's commits posts it once; it then covers all their later
contributions. A check named **CLA** on the pull request stays failed until every author has
agreed; post `recheck` to run it again. Write your commits with an e-mail address registered with
your GitHub account, so that the check can tell who wrote them. Your agreement is recorded with
your GitHub account, the name and e-mail address of your commits, the pull request and the
comment, in a private repository that only the owner can read.

karasu-slicer is developed in clean room ([AGENTS.md](AGENTS.md#clean-room-development)): a
contribution is your own work, written from observed behaviour, published specifications and
public literature. Code, pseudocode or a translation taken from any other software, open source
or commercial, is not accepted, nor is work based on reading another program's source code or
decompiling it. Third-party code enters only as a reviewed dependency.
