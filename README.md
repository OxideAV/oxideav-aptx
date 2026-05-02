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

## Compatibility — clean-room placeholders

The QMF coefficients and per-subband quantizer-interval / dither /
step-size tables in this crate are **clean-room placeholders**, not the
Qualcomm-specified values. They are chosen so that this crate's encoder
and decoder roundtrip cleanly against each other, but the on-the-wire
bytes are **not bit-identical** with FFmpeg's `aptx`/`aptx_hd` codec.

This is by design: the structural reverse-engineering trace at
`docs/audio/aptx/aptx-trace-reverse-engineering.md` deliberately omits
the numerical content of those tables (Qualcomm specifies them under
NDA, and the trace doc respects that), so a clean-room workspace cannot
ship them either.

When the real tables become available (e.g. through a published
Qualcomm spec), bit-exact interop is a drop-in swap of the constants in:

- `src/qmf.rs` — `OUTER_COEFFS`, `INNER_COEFFS`
- `src/tables.rs` — `QUANTIZATION_FACTORS`, plus the
  `make_interval_table` / `make_dither_factors` /
  `make_factor_select_offsets` helpers
- `src/subband.rs` — the predictor update rule constants

The rest of the pipeline (block layout, parity sync, channel state,
frame mux) is structurally complete.

## Status

| Component | Status |
|-----------|--------|
| 4-band QMF analysis + synthesis | implemented (placeholder coeffs) |
| Per-subband ADPCM + dither | implemented (placeholder tables) |
| Codeword pack/unpack (classic 16-bit + HD 24-bit) | implemented |
| 8-block parity-rotation sync (encode-side injection + decode-side check) | implemented |
| Self-roundtrip encoder ↔ decoder | passes |
| Bit-identical interop with FFmpeg `aptx` | **gated on Qualcomm tables** |
| Bit-identical interop with FFmpeg `aptx_hd` | **gated on Qualcomm tables** |

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
