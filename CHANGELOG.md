# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
