# Changelog

All notable changes to this crate are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `CODEC_ID_STR = "aptx"` — stable codec id for the framework
  registry. Containers tagging an aptX stream against
  `oxideav_core::CodecResolver` should reference this constant
  rather than spelling `"aptx"` inline.
- `WAVE_FORMAT_TAG_APTX: u16 = 0x0025` — RIFF/WAVE `wFormatTag`
  IANA registry assignment per RFC 2361 §A.24 (staged at
  `docs/container/riff/rfc2361-wav.txt`). Public IANA registry
  data, not bitstream-level material.
- `#[non_exhaustive]` on `Error` so future variants can land
  without a 0.x → 0.y bump for downstream consumers.
- Unit tests covering `CODEC_ID_STR` stability, the RFC 2361 tag
  value, `Error` Display / `std::error::Error` / `Clone` + `Eq`,
  and the `Result` alias.
- Crate-level docstring expanded to spell out the NDA-blocked
  status, the forbidden-cross-check policy, and the three
  realistic unblock routes (docs-collaborator transcription,
  observer-trace, or license).

### Changed

- README rewritten to mirror the lib.rs docstring: NDA status
  explicit, scaffold contents enumerated, unblock path spelled
  out.
- Clean-room rebuild from a fresh orphan `master` (round 0). The
  previous implementation was retired by the OxideAV docs audit
  dated 2026-05-06; the prior history is preserved on the `old`
  branch.
