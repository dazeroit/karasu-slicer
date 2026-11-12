# M4 grayscale PNG encoding

The encoder writes the raster's row-major eight-bit coverage samples without gamma conversion.
The PNG image is grayscale, eight-bit, noninterlaced, with filter zero on every row. No color or
transfer-function metadata is asserted. This independently designed implementation follows the
[W3C PNG specification](https://www.w3.org/TR/png-3/) and RFC 1950/1951 framing.

A virtual scanline stream inserts one zero filter byte before each source row. At most 65,535 bytes
are gathered in a fallibly reserved buffer. Each buffer becomes one stored DEFLATE block, so export
uses bounded additional memory independent of image size. Separate IDAT chunks carry the zlib
header, blocks and final Adler-32; PNG permits arbitrary splitting of the compressed stream.
CRC-32 protects each chunk, and Adler-32 protects the complete uncompressed filtered stream.
Stored blocks intentionally prioritize reproducibility, bounded memory and auditability over file
size. A future compression policy can be introduced separately without changing pixel semantics.

Dimensions must be nonzero, fit PNG's signed 31-bit dimension limit, and exactly match the sample
buffer. Arithmetic is checked before writing. Allocation, cancellation and writer failures are
structured errors. External writer failures retain their original error as the error source.
The destination may contain a partial PNG after failure or cancellation; transactional file
publication belongs to the caller. Cancellation is checked before output, once per bounded block,
and before successful completion. External write calls cannot be interrupted internally.
