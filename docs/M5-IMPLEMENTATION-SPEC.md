# M5 print jobs, extensible export and user profiles

Owner scope: CTB, GOO and the Anycubic Photon ecosystem, with Photon P1 as the first Anycubic
hardware target. This specification follows reference behaviour, public formats and the
profile-storage design. No proprietary implementation, preset library or asset is copied.

## Versioned profiles and immutable jobs

Profiles use version-one bounded UTF-8 JSON and stable portable IDs, independently of display names.
Printer hardware declares world LCD origin, physical XYZ extent, resolution, orientation, explicit
machine limits and allowed versioned exporter IDs. Resin calibration is bound to a printer ID;
there is no implicit cross-printer calibration transfer. Lengths are millimeters, durations seconds,
linear speeds millimeters/second, density grams/milliliter and optional cost per liter. Complete
profile validation is mandatory at load and job construction. The profile-storage design documents
bounds, schema rejection, locations, preset separation, optimistic revisions and atomic replacement.

A version-one PrintJob snapshots validated printer/resin profiles, complete calibration, an owned M3
uniform sample schedule and explicit sorted per-layer setting overrides. Source revisions and
calibration remain immutable after later edits. It exposes resolved per-layer metadata on demand,
avoiding a retained expanded setting allocation per layer. Jobs require 1..configured maximum layers;
bottom plus transition counts cannot exceed the job. Every nominal thickness equals calibration
thickness exactly as represented. M3 sample Z is distinct from printer Z: layer i prints at
(i+1)*thickness; it samples at the explicitly supplied M3 plane. No hidden first-plane convention is
adopted. A sample must lie within its associated printed slab, including either slab boundary.

Bottom layers use bottom settings. Transition j=1..T uses bottomExposure +
(normalExposure-bottomExposure)*j/(T+1), and normal waits/motion/PWM. Remaining layers use normal
settings. An explicit override replaces complete layer settings. Each position plus lift must fit
within printer Z travel. Exposure, waits, stage distances/speeds, PWM and all finite/range constraints
are checked for every resolved layer before export. Estimated time sums exposure, waits and
stage distance/speed; it excludes firmware acceleration, sensing, heating and extra machine work.
Geometric mask-integrated volume is an intensity-derived estimate, not measured resin consumption.

## Extensible adapters and validation

A public exporter interface separates versioned format identity, capability preflight, session start,
per-layer encoding and finalization. Built-in adapters are registered by exact ID, not guessed from
extension. Third-party application modules can register additional implementations without changing
core job representation. Machine profiles explicitly opt into IDs. CTB and Photon legacy dialects
are separate from GOO and P1 PP1; the P1 is not exported by renaming a legacy Photon stream.

Current independently implemented dialects are described in the format research/specifications:
CTB v2 plain, GOO v3, legacy Photon v1 with explicit firmware-managed motion and Anycubic P1 PP1 ZIP. A plain CTB stream is accepted only
within its documented reader-tested subset; modern encrypted-only firmware support is not inferred.
No encryption/signature bypass or vendor SDK is implemented. A backend must reject unsupported
motion, waits, overrides or scalar ranges before publication. Pixel representability is explicit:
Exact rejects loss; RoundToFormat opts into documented nearest native grayscale levels (legacy
Photon binary threshold is 128). The reference PNG path preserves the original coverage values.
No silent grayscale threshold, unsupported-field dropping or format-version upgrade is permitted.

Each input image must match job LCD geometry/orientation, contiguous index, scheduled sample Z and
nominal thickness. Too few, extra or failed source layers terminate export. Validation checks occur
before using that layer's data. A completed artifact is published only after exactly all layers and
successful source exhaustion, final cancellation check, encoder finalization and filesystem sync.

## Transactional streaming output

Machine export writes a sibling unique create_new temporary file, not the requested final path.
Default publication is create-only, with atomic no-clobber hard-link publication on the same
filesystem. Where hard links are unsupported (FAT32/exFAT removable media), publication creates the
destination exclusively and copies the staged bytes into it, then syncs it; this fallback never
replaces an existing file but is not atomic (TD-049). A future replace policy needs separate owner
intent and revision semantics. Cancellation, source/encoder/I/O errors remove only the operation's
own temporary file; preexisting user destinations are untouched. Parent directories must already
exist. File sync is attempted; directory durability is platform-dependent and documented rather
than claimed universally. Arbitrary low-level exporter sessions can leave partial writer bytes;
the high-level transactional API owns successful final publication.

Reference export creates an exclusive staging sibling directory, streams numbered grayscale PNGs
and an indexed JSONL layer manifest, writes a deterministic manifest.json last, syncs files, then
publishes the directory only to an absent destination. JSONL preserves bounded per-layer metadata;
manifest.json holds versioned job/profile snapshots and paths/counts, not an unbounded array of masks.
No timestamp, random seed or absolute user path enters completed reference bytes. Preview images
are deterministic owned generated data, with their meaning documented; no proprietary assets.

Working memory is one caller/source mask plus adapter-bounded encoded scratch, checked metadata,
and tables capped by layer count. Limits include layers, pixels per mask, encoded-layer bytes,
file bytes and metadata bytes. File size arithmetic is checked; classic ZIP/32-bit containers reject
out-of-range offsets explicitly. Cancellation polls named export checkpoints within bounded loops;
external I/O, filesystem operations, allocation and library JSON serialization are not internally
interruptible. Fixed-size bounded profile/manifest input is separate from untrusted arbitrary jobs.

## Validation and claim limits

Test profile schema/paths/revisions/backups, complete job settings and transitions, slab/sample and
travel bounds, adapter capability rejection, native mask/RLE representability, deterministic bytes,
stream errors/counts, cancellation and no-clobber publication. Use an independently authored reader
from public protocol descriptions to check produced containers, all layer pixels and metadata;
corrupt/truncated fixture tests must fail. Cross-platform byte comparison, native Windows/Linux
quality gates, an actual P1-sized mask and bounded multi-layer timing/RSS workloads are required.
Preserve initial environment failures and exact retry results. Reader acceptance does not establish
physical printing: no device or cloud/network print is authorized by file export implementation.

## Implemented boundaries

The registered IDs are `ctb-v2-plain`, `goo-v3`, `photon-v1-firmware-motion` and
`anycubic-pp1-v3`. The Photon v1 adapter explicitly delegates motion to firmware; its job motion
contributes only to the nominal estimate. CTB v2 rejects distinct waits, second motion stages and
unsupported per-layer motion/PWM changes. GOO preserves eight-bit coverage; PP1 uses sixteen native
levels. P1 support is single-material full-frame export. Dual-material mapping, physical orientation,
resin calibration and actual firmware acceptance require separate hardware validation.

Raster configuration can vary between supplied images and is recorded per layer in reference JSON.
Machine exporters declare the masks' supersampling (`BuiltinExporter::with_antialiasing`, default
one sample per pixel); every mask must use it, and the level is recorded as the GOO antialiasing
factor (grey and blur levels zero), the CTB slicer-information flag (0x0F supersampled, 0x07
binary; the header's antialiasing planes stay one, because CTB stores grey per pixel) and the PP1
`anti_count` (TD-052). Legacy Photon records no level.
`ExportSummary.coverage_volume_mm3` measures source coverage. Encoded CTB/GOO/PP1 volume instead
integrates quantized coverage; these can differ under explicit rounding. Photon v1 has no such field.

The reference directory publication checks for an absent destination before rename. Cooperative
ownership of the output parent is required: the check and directory rename cannot promise portable
race-free no-clobber behavior against another process concurrently creating an empty directory.
Machine files use atomic hard-link no-clobber publication. Profile persistence has its own exclusive
library lock and optimistic revision protocol.

See the [PP1 specification](M5-PP1-SPEC.md) and the [validation](M5-VALIDATION.md).

PP1 software provenance explicitly records the producing OS. Repeated output is deterministic on
each platform; Linux/Windows PP1 comparison permits exactly this truthful metadata difference.
Masks, scene/controller records and preview bytes must still match across platforms.
