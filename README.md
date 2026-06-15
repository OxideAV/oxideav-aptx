# oxideav-aptx

Pure-Rust aptX (classic + HD) audio codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Clean-room rebuild scaffold (NDA-blocked).** This `master` branch is
a fresh orphan. The previous implementation was retired alongside the
docs audit dated 2026-05-06 (see
[`AUDIT-2026-05-06.md`](https://github.com/OxideAV/docs/blob/master/AUDIT-2026-05-06.md)),
which found that the source-of-record trace document for this codec was
authored with a methodology that did not satisfy clean-room separation.
The prior history is preserved on the `old` branch for archival but is
forbidden input for the rebuild.

aptX (classic, HD, and Adaptive) is a Qualcomm-licensed Bluetooth
audio codec; the bitstream-level spec is distributed under NDA and
the project has not stood up a clean-room workspace at
`docs/audio/aptx*/`. Until a non-NDA spec transcription (or an
observer-trace session driven purely from pre-computed fixtures)
becomes available, the encode / decode paths cannot be safely
populated. The Implementer rounds deliberately do **not** consult
third-party reimplementations as a cross-check — those routes are
explicitly forbidden by the workspace clean-room policy
(`docs/CLEANROOM-MANUAL.md`).

The `oxideav_core::CodecResolver` registration this crate's
`register(ctx)` function will provide is wired up once the bitstream
work begins; until then the public API surfaces only the crate-local
`Error::NotImplemented` placeholder.

## What this scaffold provides

Even while NDA-blocked at the bitstream level, the crate exposes the
two NDA-safe identifiers a container demuxer / muxer needs to
recognise an aptX-tagged stream:

- `CODEC_ID_STR` (`"aptx"`) — the stable codec id the framework
  registry will route to once the bitstream lands.
- `WAVE_FORMAT_TAG_APTX` (`0x0025`) — the RIFF/WAVE `wFormatTag`
  IANA registry assignment from
  [RFC 2361 §A.24](https://www.rfc-editor.org/rfc/rfc2361)
  (staged at `docs/container/riff/rfc2361-wav.txt`). This is public
  IANA registry data, not a bitstream detail.

These constants let `oxideav-avi` / `oxideav-riff` (etc.) tag aptX
streams as `"aptx"` against `oxideav-core`'s `CodecResolver` without
taking a hard dep on a populated decoder — the resolver maps tag →
codec id, the codec returns `Error::NotImplemented` until the
bitstream is unblocked, but at least the *container* side reports
the right codec instead of "unknown".

## Unblock path

The bitstream paths remain blocked. The realistic unblock routes
are, in order of preference:

1. A docs-collaborator round that transcribes the QMF coefficients
   and the four subband quantiser tables from a public-domain or
   permissively-licensed primary source (RFC, ITU recommendation,
   academic paper that pre-dates Qualcomm's NDA umbrella) into
   `docs/audio/aptx/tables/`.
2. A clean-room observer-trace session that records the input /
   output PCM of an opaque encoder binary on a fixture corpus
   without reading its source — analogous to the JPEG-XL
   reference-encoder trace methodology already used in
   `docs/image/jpegxl/`.
3. License negotiation. Not pursued in this workspace.
