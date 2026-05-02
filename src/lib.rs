//! Pure-Rust **aptX classic** + **aptX HD** Bluetooth audio codec.
//!
//! aptX is Qualcomm's constant-bit-rate sub-band ADPCM stereo codec
//! used by the Bluetooth A2DP profile. Both variants share the same
//! pipeline (two-stage dyadic QMF + per-(channel, subband) Jayant
//! ADPCM + 8-block parity-rotation sync); they differ only in the
//! per-subband bit allocation:
//!
//! | Subband | classic bits | HD bits |
//! |---------|-------------:|--------:|
//! | LF      |      7       |    9    |
//! | MLF     |      4       |    6    |
//! | MHF     |      2       |    4    |
//! | HF      |      3       |    5    |
//! | total   |     16       |   24    |
//!
//! Wire format is headerless: back-to-back 4 B (classic) or 6 B (HD)
//! big-endian stereo blocks, no CRC, no length, no sample-rate
//! signalling. See the trace doc at
//! `docs/audio/aptx/aptx-trace-reverse-engineering.md` for the full
//! pipeline shape this implementation follows.
//!
//! # Compatibility
//!
//! The QMF coefficients and per-subband quantizer-interval / dither /
//! step-size tables are **clean-room placeholders** chosen for
//! self-roundtrip stability. Bit-exact interop with FFmpeg's
//! `aptx`/`aptx_hd` requires the Qualcomm-specified numerical tables,
//! which are NDA-only and which the trace doc deliberately omits.
//! When those tables become available, swap them into [`tables`] and
//! [`qmf`] — the rest of the pipeline is structurally complete.
//!
//! # Quick use
//!
//! ```no_run
//! use oxideav_core::{
//!     AudioFrame, CodecId, CodecParameters, Frame, SampleFormat,
//! };
//! use oxideav_core::{Decoder, Encoder};
//!
//! let mut params = CodecParameters::audio(CodecId::new(oxideav_aptx::CODEC_ID_CLASSIC));
//! params.sample_rate = Some(44_100);
//! params.channels = Some(2);
//! params.sample_format = Some(SampleFormat::S16);
//!
//! let mut enc = oxideav_aptx::encoder::make_encoder(&params).unwrap();
//! let mut dec = oxideav_aptx::decoder::make_decoder(&params).unwrap();
//!
//! // Pump 4 stereo PCM samples (8 interleaved s16) through the encoder.
//! let pcm: Vec<i16> = (0..1024).map(|i| ((i as f32 * 0.1).sin() * 8_000.0) as i16).collect();
//! let mut bytes = Vec::with_capacity(pcm.len() * 2);
//! for &s in &pcm { bytes.extend_from_slice(&s.to_le_bytes()); }
//! let frame = Frame::Audio(AudioFrame {
//!     samples: (pcm.len() / 2) as u32,
//!     pts: Some(0),
//!     data: vec![bytes],
//! });
//! enc.send_frame(&frame).unwrap();
//! enc.flush().unwrap();
//! while let Ok(pkt) = enc.receive_packet() {
//!     dec.send_packet(&pkt).unwrap();
//! }
//! ```

#![allow(clippy::needless_range_loop, clippy::doc_lazy_continuation)]

pub mod channel;
pub mod codeword;
pub mod decoder;
pub mod dither;
pub mod encoder;
pub mod qmf;
pub mod subband;
pub mod tables;

use oxideav_core::{CodecCapabilities, CodecId, CodecInfo, CodecRegistry};

/// Codec id string for aptX classic.
pub const CODEC_ID_CLASSIC: &str = "aptx";
/// Codec id string for aptX HD.
pub const CODEC_ID_HD: &str = "aptx_hd";
/// Default codec id string (classic).
pub const CODEC_ID_STR: &str = CODEC_ID_CLASSIC;

/// Register both aptX variants (classic + HD) in `reg`.
pub fn register(reg: &mut CodecRegistry) {
    let caps = CodecCapabilities::audio("aptx_sw")
        .with_lossy(true)
        .with_intra_only(false)
        .with_max_channels(2);
    reg.register(
        CodecInfo::new(CodecId::new(CODEC_ID_CLASSIC))
            .capabilities(caps.clone())
            .decoder(decoder::make_decoder)
            .encoder(encoder::make_encoder),
    );
    let caps_hd = CodecCapabilities::audio("aptx_hd_sw")
        .with_lossy(true)
        .with_intra_only(false)
        .with_max_channels(2);
    reg.register(
        CodecInfo::new(CodecId::new(CODEC_ID_HD))
            .capabilities(caps_hd)
            .decoder(decoder::make_decoder)
            .encoder(encoder::make_encoder),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use oxideav_core::Encoder;
    use oxideav_core::{CodecParameters, SampleFormat};

    fn classic_params() -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_CLASSIC));
        p.sample_rate = Some(44_100);
        p.channels = Some(2);
        p.sample_format = Some(SampleFormat::S16);
        p
    }

    fn hd_params() -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_HD));
        p.sample_rate = Some(48_000);
        p.channels = Some(2);
        p.sample_format = Some(SampleFormat::S16);
        p
    }

    #[test]
    fn registers_both_variants_both_directions() {
        let mut reg = CodecRegistry::new();
        register(&mut reg);
        for id in [CODEC_ID_CLASSIC, CODEC_ID_HD] {
            let cid = CodecId::new(id);
            assert!(reg.has_decoder(&cid), "no decoder for {id}");
            assert!(reg.has_encoder(&cid), "no encoder for {id}");
        }
    }

    #[test]
    fn rejects_mono() {
        let mut p = classic_params();
        p.channels = Some(1);
        assert!(decoder::make_decoder(&p).is_err());
        assert!(encoder::make_encoder(&p).is_err());
    }

    #[test]
    fn rejects_unknown_sample_rate() {
        let mut p = classic_params();
        p.sample_rate = Some(22_050);
        assert!(decoder::make_decoder(&p).is_err());
        assert!(encoder::make_encoder(&p).is_err());
    }

    #[test]
    fn classic_bit_rate_at_44k1_is_352800() {
        let enc = encoder::make_encoder(&classic_params()).unwrap();
        assert_eq!(enc.output_params().bit_rate, Some(352_800));
    }

    #[test]
    fn hd_bit_rate_at_48k_is_576000() {
        let enc = encoder::make_encoder(&hd_params()).unwrap();
        assert_eq!(enc.output_params().bit_rate, Some(576_000));
    }
}
