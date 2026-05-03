# oxideav-aptx

Pure-Rust **aptX classic** + **aptX HD** Bluetooth audio codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

aptX is Qualcomm's constant-bit-rate sub-band ADPCM stereo audio codec
used by the Bluetooth A2DP profile. Both variants share the same
pipeline (two-stage dyadic QMF + per-(channel, subband) Jayant ADPCM +
8-block parity-rotation in-band sync); they differ only in the
per-subband bit allocation and the corresponding quantizer-table sizes.

| Subband | Classic bits | HD bits |
|---------|-------------:|--------:|
| LF      |      7       |    9    |
| MLF     |      4       |    6    |
| MHF     |      2       |    4    |
| HF      |      3       |    5    |
| total   |     16       |   24    |

Wire format is headerless: back-to-back 4 B (classic) or 6 B (HD)
big-endian stereo blocks. No CRC, no length, no sample-rate signalling.
The 8-block parity-rotation on the HF LSB carries the entire
synchronization story.

## Compatibility

Round 2 (May 2026) replaced the round-1 clean-room placeholders with
the spec-shipped numerical data published in
[`docs/audio/aptx/data/aptx-qmf-coefficients.md`](https://github.com/OxideAV/oxideav-workspace/blob/master/docs/audio/aptx/data/aptx-qmf-coefficients.md)
and
[`docs/audio/aptx/data/aptx-quantizer-tables.md`](https://github.com/OxideAV/oxideav-workspace/blob/master/docs/audio/aptx/data/aptx-quantizer-tables.md).
The same numerical values appear byte-identically in two unrelated
open-source projects (FFmpeg `libavcodec/aptx.{c,h}` and libopenaptx
by Pali Rohár); the sidecars reproduce them as functional public
data.

Concretely, the active spec content is:

- `src/qmf.rs` — 16-tap outer + inner QMF coefficient sets + the
  spec analysis/synthesis right-shifts (23 / 21 / 23 / 22).
- `src/tables.rs` — every per-(variant, subband) `quantize_intervals`,
  `invert_quantize_dither_factors`, `quantize_dither_factors`, and
  `quantize_factor_select_offset` table from the sidecar; the common
  32-entry `QUANTIZATION_FACTORS`; and the per-subband `factor_max`
  caps (`0x11FF`, `0x14FF`, `0x16FF`, `0x15FF`).
- `src/subband.rs` — the spec leaky-integrator `factor_select` update
  (leak constant `32620 / 32768`) and the spec quantization-factor
  lookup `(QUANTIZATION_FACTORS[(fs & 0xFF) >> 3] << 11) >>
  ((factor_max − fs) >> 8)`.

The dither output mapping in `src/dither.rs` still uses an LFSR
mixer; the codeword-history *update* equation matches the spec but
the per-subband output extraction is approximate (the spec's `×
5_184_443` post-shift bit-position decomposition is a follow-on
refinement). Combined with small residual differences in the
predictor short-term constants and the parity-injection visit order,
this is what keeps the FFmpeg-interop path from reaching full
bit-exactness in this round.

## Status

| Component | Status |
|-----------|--------|
| 4-band QMF analysis + synthesis (16-tap, spec coefficients) | implemented |
| Per-subband ADPCM + spec quantizer tables | implemented |
| Per-subband `factor_max` caps + spec leaky-integrator update | implemented |
| Spec quantization-factor lookup (`(QF[idx] << 11) >> shift`) | implemented |
| Codeword pack/unpack (classic 16-bit + HD 24-bit) | implemented |
| 8-block parity-rotation sync (encode-side injection + decode-side check) | implemented |
| Self-roundtrip encoder ↔ decoder (~29 dB PSNR @ 500 Hz) | passes |
| Decoder of FFmpeg `aptx` stream produces signal (PSNR > 0 dB, was −12 dB) | passes |
| Bit-identical interop with FFmpeg `aptx` / `aptx_hd` | needs spec dither output rule (round 3) |

## Installation

```toml
[dependencies]
oxideav-core = "0.1"
oxideav-aptx = "0.0"
```

## Quick use

```rust,no_run
use oxideav_core::{AudioFrame, CodecId, CodecParameters, Frame, SampleFormat};
use oxideav_core::{Decoder, Encoder};

let mut params = CodecParameters::audio(CodecId::new(oxideav_aptx::CODEC_ID_CLASSIC));
params.sample_rate = Some(44_100);
params.channels = Some(2);
params.sample_format = Some(SampleFormat::S16);

let mut enc = oxideav_aptx::encoder::make_encoder(&params).unwrap();
let mut dec = oxideav_aptx::decoder::make_decoder(&params).unwrap();

// Stereo S16LE input — pump it in, drain encoded packets, feed decoder.
let pcm: Vec<i16> = (0..4096)
    .map(|i| ((i as f32 * 0.05).sin() * 8_000.0) as i16)
    .collect();
let mut bytes = Vec::with_capacity(pcm.len() * 2);
for &s in &pcm { bytes.extend_from_slice(&s.to_le_bytes()); }
enc.send_frame(&Frame::Audio(AudioFrame {
    samples: (pcm.len() / 2) as u32,
    pts: Some(0),
    data: vec![bytes],
}))
    .unwrap();
enc.flush().unwrap();
while let Ok(pkt) = enc.receive_packet() {
    dec.send_packet(&pkt).unwrap();
}
```

## Trace doc

The pipeline reconstruction this crate follows lives at
[`docs/audio/aptx/aptx-trace-reverse-engineering.md`](https://github.com/OxideAV/oxideav-workspace/blob/master/docs/audio/aptx/aptx-trace-reverse-engineering.md)
in the parent workspace.

## License

MIT — see [LICENSE](LICENSE).
