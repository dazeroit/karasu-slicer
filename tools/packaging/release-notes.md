## Downloads

| System | File |
|---|---|
| Windows 10 or 11, x64: installer | `karasu-slicer-{version}-windows-x64-setup.exe` |
| Windows 10 or 11, x64: portable | `karasu-slicer-{version}-windows-x64.zip` |
| Debian, Ubuntu (x86-64) | `karasu-slicer_{version}_amd64.deb` |
| Other Linux distributions (x86-64) | `karasu-slicer-{version}-linux-x86_64.tar.gz`, then `./install.sh` |

The Windows files are not signed. Windows may say "Windows protected your PC": choose
**More info**, then **Run anyway**.

## Checking a download

Every file is listed in `SHA256SUMS`, and GitHub attests that it was built from this release's
tag:

```console
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify <file> --repo dazeroit/karasu-slicer
```
