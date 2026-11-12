# Code-signing feasibility (M6.7)

**Status:** research record, 2026-11-10, at the owner's request (M6.7 "signing feasibility";
[M6-PLAN](../M6-PLAN.md), "Code signing"). No decision is taken here: the choice depends on two
owner decisions listed at the end. **Owner, 2026-11-10:** no signing for now; when it comes, the
publisher is an individual. Builds stay unsigned (TD-141).

**Method:** public documentation from Microsoft, certificate authorities and signing services,
read on 2026-11-10; prices and rules change, so each figure names its source and is to be
checked again before buying. Observations are marked apart from inferences.

## What is signed

- **Windows:** `karasu.exe`, `karasu-cli.exe`, the Inno Setup installer and its uninstaller
  (Authenticode, SHA-256, with an RFC 3161 timestamp so signatures stay valid after the
  certificate expires). The ZIP itself is not signed; its executables are.
- **Linux:** the tar.gz and the `.deb`, and a `SHA256SUMS` list of every release file.

## Windows: the rules that apply (observations)

- **SmartScreen** looks at two signals: the file hash's reputation and the signing publisher's
  reputation. A new file, signed or not, can show a warning until reputation builds ("several
  weeks and hundreds of clean installs"). Unsigned files start from zero with every release;
  files signed with the same publisher identity let the publisher's reputation carry over to
  new versions. *Source: Microsoft Learn, "SmartScreen reputation for Windows app developers"
  (updated 2026-08-17).*
- **EV certificates no longer bypass SmartScreen** (since 2024): EV and OV behave the same;
  paying for EV to avoid warnings "is no longer justified". *Same source, and Microsoft Learn,
  "Code signing options for Windows app developers" (updated 2026-08-29).*
- **Smart App Control** (Windows 11) may block unsigned files without positive reputation,
  whether downloaded or not. *SmartScreen page above.* This matters more than SmartScreen for
  an unsigned application: on such systems it may not start at all.
- **Keys on hardware:** since June 2023 the private keys of publicly trusted code-signing
  certificates must be on a hardware token or HSM (FIPS 140-2 level 2 or Common Criteria EAL 4+);
  CAs offer USB tokens or cloud HSMs. *Microsoft code-signing options page; CA pages.*
- **Validity:** certificates issued from 1 March 2026 last at most 460 days (CA/B Forum ballot
  CSC-31; Sectigo states 459 days from 23 February 2026). A multi-year purchase means reissues.
  *DigiCert and Sectigo notices; the ballot text itself was not read.*

## Windows: options

| Option | Cost (source) | Who can use it | Keys | Notes |
| --- | --- | --- | --- | --- |
| **Azure Artifact Signing** (formerly Trusted Signing) | about $9.99 a month, 5,000 signatures (Microsoft) | Organizations in the US, Canada, EU, UK and some other countries; **individuals only in the US and Canada** (Microsoft quickstart, 2026-10-08) | Held by Microsoft; signing from CI with an Azure identity | Microsoft's recommended path outside the Store; needs a paid Azure subscription; identity validation 1–20 business days |
| **OV certificate from a CA, cloud key** | Certum "Standard Code Signing in the Cloud" from €209 (Certum shop); Microsoft quotes $150–300 a year in general | Worldwide; individual issuance to be confirmed with the CA | Cloud HSM (Certum SimplySign) or USB token | Works for an individual in Italy if the CA issues to natural persons |
| **Certum Open Source Code Signing (cloud)** | from €49 (Certum shop; "out of stock" on 2026-11-10) | Individual developers of free or open-source software | SimplySign cloud, 5,000 signatures a month | Subject reads "Open Source Developer" and the person's name; needs the project to be open source |
| **SignPath Foundation** | free | OSI-licensed projects without commercial dual licensing, public repository, automated builds | Held by SignPath; signing from CI | The publisher shown is SignPath Foundation, not the owner; needs a code-signing policy page and MFA for the team |
| **EV certificate** | from €379 (Certum shop), $400+ (Microsoft) | Organizations | Token or cloud | No SmartScreen benefit any more; only for enterprise procurement |
| **Microsoft Store (MSIX)** | free | Worldwide | Microsoft re-signs | No SmartScreen warning at all; needs an MSIX package and a Store listing, another packaging route |
| **Unsigned** (today) | free | — | — | "Windows protected your PC" on download; Smart App Control may block; more antivirus false positives (TD-063 record) |

**Tools (inference from vendor documentation):** signing runs where the Windows build runs.
`signtool` (Windows SDK) signs with a cloud key through the vendor's provider (Artifact Signing's
dlib, SimplySign Desktop, eSigner); `osslsigncode` 2.5+ and Jsign sign from Linux through PKCS#11
and timestamp, if a later pipeline builds there. Inno Setup signs the installer and its
uninstaller itself (`SignTool=` with `SignedUninstaller=yes`), so `tools/packaging/karasu.iss`
needs one directive and a configured sign tool.

**Reproducibility:** a signature changes the executable's bytes. Packages stay reproducible from
the same signed executables; the release record keeps the hashes of both the unsigned build and
the signed files.

## Linux: options

- **A signed checksum list** for every release: `SHA256SUMS` with a detached signature
  (OpenPGP with GnuPG or Sequoia, or minisign). Free; the public key or its fingerprint is
  published on the project's site and in its documentation. This covers the tar.gz and the `.deb`
  alike and is what users of a downloaded package can check.
- **The `.deb` itself:** signatures inside a `.deb` (debsigs, dpkg-sig, incompatible with each
  other) are not checked by `dpkg` or APT by default. APT checks signatures of a *repository*
  (`InRelease`, `Release.gpg`) against a key the user adds with `signed-by=`. Signing therefore
  becomes meaningful for `.deb` users when the project publishes an APT repository; until then
  the signed `SHA256SUMS` serves. *Sources: `apt-secure(8)`, Debian Handbook.*
- **Later routes:** Flathub signs Flatpaks itself; an APT repository can be hosted with the
  release files. Sigstore (keyless signatures tied to a CI identity) fits when releases are
  built in public CI.

**Key custody (Linux):** an offline OpenPGP primary key with a signing subkey on a hardware key
(for example a YubiKey or a Nitrokey), a revocation certificate kept apart, and the fingerprint
published. Cost: a hardware key, about €50–60, once.

## Costs at a glance (first year, indicative)

| Route | Windows | Linux | Total |
| --- | --- | --- | --- |
| Organization in the EU | Artifact Signing, about $120 a year | signed checksums, hardware key once | about €110 + €55 |
| Individual, project open source | SignPath Foundation (free) or Certum Open Source (€49) | same | €0–49 + €55 |
| Individual, project not open source | OV in the cloud (Certum Standard from €209) | same | about €209 + €55 |

## Recommendation (inference)

1. **Sign Windows releases** before the first public release (M7): unsigned, Smart App Control
   may refuse to start the application on Windows 11, and every release starts with no
   reputation. EV is not worth its price.
2. **The route follows the two owner decisions below:** Artifact Signing for an organization in
   the EU (cheapest, no token, CI-friendly); for an individual, SignPath or Certum Open Source if
   the project is open source, an OV certificate in the cloud otherwise.
3. **Linux:** a signed `SHA256SUMS` from the first release; an APT repository only when users
   ask for updates through APT.
4. **Microsoft Store (MSIX)** as a later, additional channel: free signing and no warnings, at
   the cost of a second Windows package.

## Owner decisions needed

- **Who publishes:** decided (owner, 2026-11-10): an individual, the name in the certificate
  being the owner's. Artifact Signing, open to organizations only, is out; the routes left are
  SignPath or Certum Open Source if the project is open source, an OV certificate in the cloud
  otherwise. No signing for now.
- **The project's licence:** decided (owner, 2026-11-11, TD-145): AGPL-3.0-or-later, OSI
  approved, with a CLA that keeps commercial dual licensing possible. SignPath Foundation
  excludes commercial dual licensing, so the open-source route left is Certum Open Source.
- **When:** decided (owner, 2026-11-12, TD-150): no certificate until the public response
  justifies its cost; previews and the first releases are published unsigned, with
  `SHA256SUMS` and a build provenance attestation.

## Sources (read 2026-11-10)

- Microsoft Learn, [Code signing options for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) (updated 2026-08-29).
- Microsoft Learn, [SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) (updated 2026-08-17).
- Microsoft Learn, [Quickstart: Set up Artifact Signing](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) (updated 2026-10-08).
- Certum, [Open Source Code Signing in the Cloud](https://shop.certum.eu/open-source-code-signing-on-simplysign.html) and [Buy a Code Signing certificate](https://shop.certum.eu/buy-a-code-signing-certyficate).
- DigiCert, [Understanding the new code signing certificate validity change](https://www.digicert.com/blog/understanding-the-new-code-signing-certificate-validity-change); Sectigo, [Code signing](https://sectigo.com/signing-certificates/code-signing).
- SignPath Foundation, [signpath.org](https://signpath.org/) and its [terms](https://signpath.org/terms).
- DigiCert, [osslsigncode with PKCS#11](https://docs.digicert.com/en/software-trust-manager/signing-tools/osslsigncode-with-pkcs11.html); [Jsign](https://ebourg.github.io/jsign).
- `apt-secure(8)` ([manpage](https://manpages.ubuntu.com/manpages/plucky/man8/apt-secure.8.html)); Debian Handbook, package authentication.
