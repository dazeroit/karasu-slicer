# Security policy

## Reporting a vulnerability

Report vulnerabilities privately through GitHub: on the repository's **Security** tab, choose
**Report a vulnerability**. Do not open a public issue, pull request or discussion about it.

Please include:

- the karasu-slicer version or commit, and the operating system;
- what an attacker can do, and the conditions it needs;
- the steps or a minimal input file that reproduces it, if you can share one;
- whether the issue is already public or known to others.

The owner acknowledges the report, keeps you informed while it is investigated and fixed, and
agrees with you when it is disclosed. You are credited in the advisory unless you prefer not to
be.

## Supported versions

| Version | Supported |
|---|---|
| `main` | yes |
| 0.x previews | no: fixes ship in the next preview |

karasu-slicer has no stable release yet. From 1.0, the latest series receives every fix and the
previous series receives security fixes for three months after the first release of the next
major version ([docs/RELEASE-PROCESS.md](docs/RELEASE-PROCESS.md#supported-series)); this table
lists them.

## Scope

karasu-slicer reads files that may come from untrusted sources, so these matter most:

- model files (STL, OBJ, 3MF) and project files: crashes, unbounded memory or time, or writes
  outside the chosen output;
- printer, resin and print-setting profiles and libraries;
- the machine files it writes, when they could harm a printer or its firmware;
- the desktop application and its installer on Windows and Linux.

Bugs without a security impact, such as wrong slicing results, belong in ordinary issues.
