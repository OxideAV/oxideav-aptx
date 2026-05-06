# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2](https://github.com/OxideAV/oxideav-aptx/compare/v0.0.1...v0.0.2) - 2026-05-06

### Other

- prepend retirement notice (docs audit 2026-05-06)

### Changed

- replaced placeholder Haar 2-tap QMF with the spec 16-tap two-stage
  cascade from `docs/audio/aptx/data/aptx-qmf-coefficients.md`
  (outer + inner, mirror-paired filter sets, 23 / 21 / 23 / 22
  rounded shifts)
- replaced placeholder quadratic-spaced quantizer tables with the
  spec-shipped `quantize_intervals`, `invert_quantize_dither_factors`,
  `quantize_dither_factors`, and `quantize_factor_select_offset` tables
  for both classic and HD per `docs/audio/aptx/data/aptx-quantizer-tables.md`
- adopted the spec per-subband `factor_max` caps (`0x11FF / 0x14FF /
  0x16FF / 0x15FF`) and the spec quantization-factor lookup
  `(QUANTIZATION_FACTORS[(fs & 0xFF) >> 3] << 11) >> ((factor_max −
  fs) >> 8)` in [`subband`]
- adopted the spec `factor_select` leaky-integrator update with leak
  constant `32620 / 32768`
- subband encoder now scales the diff search by `<< 19` to mirror the
  spec decoder's `(qf × qr) >> 19` reconstruction-difference path
- dither generator's codeword-history *update* equation now matches
  the spec (`history = (history << 4) | field` with field built from
  `LF[0] | LF[1] | MLF[1] | MHF[0]`); the per-subband dither output
  mapping is still an LFSR mixer pending a round-3 follow-up

### Added

- spec sanity-check tests (mirror-pair property, dominant tap, DC
  sums-agree, factor_max caps, dither field packing)
- subband self-roundtrip tracking test (PSNR > 30 dB on a slow tone)

### Performance

- self-roundtrip PSNR @ 500 Hz: 22 dB → 29 dB (classic), 21 dB →
  29 dB (HD)
- ffmpeg-encoded stream decode PSNR: −12 dB → +4 dB (still well
  short of bit-exact; gap is the dither output mapping + a few
  predictor constants)

## [0.0.1] - 2026-05-02

### Added

- initial scaffold: aptX classic (4 B/block) decoder + encoder
- 4-band two-stage dyadic QMF (analysis + synthesis), placeholder
  clean-room coefficients
- per-(channel, subband) backward-adaptive Jayant-style ADPCM with
  the LF/MLF/MHF/HF prediction orders observed in the trace
  (24 / 12 / 6 / 12)
- codeword packing + 8-block parity-rotation sync (HF LSB carrier)
- aptX HD (6 B/block) deferred to round 2 — pipeline is the same,
  only the per-subband bit allocation and table sizes change
- self-roundtrip tone test (PSNR floor)

### Status

Bit-exact interop with FFmpeg's `aptx` encoder/decoder is gated on
the Qualcomm-specified quantizer-interval tables, dither tables,
and QMF coefficient sets, which are NDA-only. This crate ships
clean-room placeholder tables that yield a self-consistent encoder
↔ decoder pair but produce a different on-the-wire byte stream
from the upstream reference. See README §Compatibility.
