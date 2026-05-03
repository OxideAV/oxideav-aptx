//! End-to-end encoder ↔ decoder roundtrip for both aptX variants.
//!
//! Because this crate ships clean-room placeholder QMF coefficients
//! and quantizer tables (see `tables.rs` and `qmf.rs`), the on-the-wire
//! bytes are NOT identical to FFmpeg's `aptx`/`aptx_hd` codec. What
//! we verify here is that the encoder and decoder shipped together
//! self-roundtrip a tone with non-trivial fidelity — the structural
//! pipeline (QMF → ADPCM → parity sync → pack/unpack → ADPCM → QMF)
//! is exercised end-to-end.

use oxideav_core::{AudioFrame, CodecId, CodecParameters, Frame, SampleFormat};
#[allow(unused_imports)]
use oxideav_core::{Decoder, Encoder};

fn classic_params(sample_rate: u32) -> CodecParameters {
    let mut p = CodecParameters::audio(CodecId::new(oxideav_aptx::CODEC_ID_CLASSIC));
    p.sample_rate = Some(sample_rate);
    p.channels = Some(2);
    p.sample_format = Some(SampleFormat::S16);
    p
}

fn hd_params(sample_rate: u32) -> CodecParameters {
    let mut p = CodecParameters::audio(CodecId::new(oxideav_aptx::CODEC_ID_HD));
    p.sample_rate = Some(sample_rate);
    p.channels = Some(2);
    p.sample_format = Some(SampleFormat::S16);
    p
}

/// Build an interleaved s16 stereo sine of `len_pairs` PCM frames at
/// `freq` Hz.
fn sine_stereo(len_pairs: usize, freq: f32, sample_rate: u32, amp: f32) -> Vec<i16> {
    let two_pi = 2.0_f32 * std::f32::consts::PI;
    let mut out = Vec::with_capacity(len_pairs * 2);
    for n in 0..len_pairs {
        let t = n as f32 / sample_rate as f32;
        let v = ((two_pi * freq * t).sin() * amp) as i16;
        out.push(v); // left
        out.push(v); // right (mono → dual-mono)
    }
    out
}

fn audio_frame(samples: &[i16]) -> Frame {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    Frame::Audio(AudioFrame {
        samples: (samples.len() / 2) as u32,
        pts: Some(0),
        data: vec![bytes],
    })
}

/// Encode then decode `pcm` through one side of the codec and return
/// the recovered interleaved s16 samples.
fn roundtrip(params: &CodecParameters, pcm: &[i16]) -> Vec<i16> {
    let mut enc = oxideav_aptx::encoder::make_encoder(params).expect("encoder");
    let mut dec = oxideav_aptx::decoder::make_decoder(params).expect("decoder");
    enc.send_frame(&audio_frame(pcm)).expect("send_frame");
    enc.flush().expect("flush");
    let mut decoded: Vec<i16> = Vec::new();
    while let Ok(pkt) = enc.receive_packet() {
        dec.send_packet(&pkt).expect("send_packet");
        loop {
            match dec.receive_frame() {
                Ok(Frame::Audio(af)) => {
                    for chunk in af.data[0].chunks_exact(2) {
                        decoded.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                    }
                }
                Ok(_) => break,
                Err(oxideav_core::Error::NeedMore) => break,
                Err(oxideav_core::Error::Eof) => break,
                Err(e) => panic!("decode error: {e}"),
            }
        }
    }
    dec.flush().ok();
    decoded
}

/// Best-shift PSNR (dB) between `original` and `decoded`. Considers
/// shifts 0..=256 to absorb the cascaded QMF + ADPCM startup latency
/// (the two-stage 16-tap QMF cascade alone has a group delay of ~66
/// samples per channel; the full encode→decode round-trip stacks two
/// of those plus the ADPCM convergence transient).
fn psnr(original: &[i16], decoded: &[i16]) -> f64 {
    let n_total = original.len().min(decoded.len());
    let skip_head = (n_total / 4).min(512);
    let mut best = f64::NEG_INFINITY;
    for delay in 0..256 {
        if skip_head + delay >= n_total {
            continue;
        }
        let n = n_total - skip_head - delay;
        let mut err = 0.0f64;
        let mut sig = 0.0f64;
        for i in 0..n {
            let x = original[skip_head + i] as f64;
            let y = decoded[skip_head + delay + i] as f64;
            err += (x - y).powi(2);
            sig += x * x;
        }
        let v = if err == 0.0 {
            200.0
        } else if sig == 0.0 {
            0.0
        } else {
            10.0 * (sig / err).log10()
        };
        if v > best {
            best = v;
        }
    }
    best
}

#[test]
fn classic_self_roundtrip_500hz_44k1() {
    // 500 Hz, 0.5 s stereo at 44.1 kHz. With the spec QMF coefficients
    // and quantizer tables the self-roundtrip PSNR clears the
    // ~25 dB classic envelope.
    let pcm = sine_stereo(22050, 500.0, 44_100, 8_000.0);
    let decoded = roundtrip(&classic_params(44_100), &pcm);
    assert!(
        decoded.len() >= pcm.len() / 2,
        "decoder produced only {} samples vs {} input",
        decoded.len(),
        pcm.len()
    );
    let snr = psnr(&pcm, &decoded);
    eprintln!("aptX classic 500 Hz @ 44.1 kHz self-roundtrip PSNR = {snr:.2} dB");
    assert!(
        snr > 25.0,
        "PSNR {snr:.2} dB below 25 dB classic self-roundtrip envelope"
    );
}

#[test]
fn hd_self_roundtrip_500hz_48k() {
    // 500 Hz, 0.5 s stereo at 48 kHz. HD's wider quantizer codewords
    // give it ~5-10 dB more PSNR than classic; the assertion is loose
    // enough to be a stable regression-only check.
    let pcm = sine_stereo(24000, 500.0, 48_000, 8_000.0);
    let decoded = roundtrip(&hd_params(48_000), &pcm);
    assert!(
        decoded.len() >= pcm.len() / 2,
        "decoder produced only {} samples vs {} input",
        decoded.len(),
        pcm.len()
    );
    let snr = psnr(&pcm, &decoded);
    eprintln!("aptX HD 500 Hz @ 48 kHz self-roundtrip PSNR = {snr:.2} dB");
    assert!(
        snr > 25.0,
        "PSNR {snr:.2} dB below 25 dB HD self-roundtrip envelope"
    );
}

#[test]
fn classic_silence_roundtrip_stays_bounded() {
    let pcm = vec![0i16; 4096]; // 1024 stereo PCM frames
    let decoded = roundtrip(&classic_params(44_100), &pcm);
    assert!(!decoded.is_empty());
    // After the startup transient, decoded silence should stay near 0.
    // Allow a generous bound for the placeholder ADPCM (the dither LFSR
    // produces small but non-zero noise even on silent input).
    let head = 64;
    for (i, &s) in decoded.iter().enumerate().skip(head) {
        assert!(
            (s as i32).abs() < 4_000,
            "silence drifted at sample {i}: {s} (limit 4000)"
        );
    }
}

#[test]
fn classic_block_size_is_4_bytes() {
    // Verify the on-the-wire layout: 4 PCM frames stereo → exactly 4
    // bytes for aptX classic.
    let pcm = sine_stereo(16, 1000.0, 44_100, 4000.0); // 16 stereo frames = 4 blocks
    let mut enc = oxideav_aptx::encoder::make_encoder(&classic_params(44_100)).expect("encoder");
    enc.send_frame(&audio_frame(&pcm)).unwrap();
    enc.flush().unwrap();
    let mut total = 0;
    while let Ok(pkt) = enc.receive_packet() {
        total += pkt.data.len();
    }
    assert_eq!(total, 16, "16 stereo PCM frames → 4 blocks × 4 B each");
}

#[test]
fn hd_block_size_is_6_bytes() {
    // 16 stereo frames = 4 blocks @ 6 B each = 24 B.
    let pcm = sine_stereo(16, 1000.0, 48_000, 4000.0);
    let mut enc = oxideav_aptx::encoder::make_encoder(&hd_params(48_000)).expect("encoder");
    enc.send_frame(&audio_frame(&pcm)).unwrap();
    enc.flush().unwrap();
    let mut total = 0;
    while let Ok(pkt) = enc.receive_packet() {
        total += pkt.data.len();
    }
    assert_eq!(total, 24, "16 stereo PCM frames → 4 blocks × 6 B each");
}

#[test]
fn parity_sync_clean_stream_no_errors() {
    // A clean encoded stream must produce zero parity errors at the
    // decoder.
    let pcm = sine_stereo(2048, 1000.0, 44_100, 8000.0);
    let mut enc = oxideav_aptx::encoder::make_encoder(&classic_params(44_100)).expect("encoder");
    enc.send_frame(&audio_frame(&pcm)).unwrap();
    enc.flush().unwrap();
    let mut all_bytes = Vec::new();
    while let Ok(pkt) = enc.receive_packet() {
        all_bytes.extend_from_slice(&pkt.data);
    }
    // Decode using the concrete decoder so we can read parity_errors.
    let mut dec =
        oxideav_aptx::decoder::AptxDecoder::new(oxideav_aptx::tables::Variant::Classic, 44_100);
    let mut pkt = oxideav_core::Packet::new(0, oxideav_core::TimeBase::new(1, 44_100), all_bytes);
    pkt.flags.keyframe = true;
    dec.send_packet(&pkt).unwrap();
    assert_eq!(
        dec.parity_errors(),
        0,
        "clean encoded stream produced {} parity errors at the decoder",
        dec.parity_errors()
    );
}

#[test]
fn parity_sync_corrupted_stream_flags_errors() {
    // Shift the byte alignment by 1 byte midway through and confirm
    // the decoder raises parity errors.
    let pcm = sine_stereo(2048, 1000.0, 44_100, 8000.0);
    let mut enc = oxideav_aptx::encoder::make_encoder(&classic_params(44_100)).expect("encoder");
    enc.send_frame(&audio_frame(&pcm)).unwrap();
    enc.flush().unwrap();
    let mut all_bytes = Vec::new();
    while let Ok(pkt) = enc.receive_packet() {
        all_bytes.extend_from_slice(&pkt.data);
    }
    // Inject a 1-byte shift after offset 100.
    if all_bytes.len() > 200 {
        all_bytes.insert(100, 0xAA);
    }
    let mut dec =
        oxideav_aptx::decoder::AptxDecoder::new(oxideav_aptx::tables::Variant::Classic, 44_100);
    let mut pkt = oxideav_core::Packet::new(0, oxideav_core::TimeBase::new(1, 44_100), all_bytes);
    pkt.flags.keyframe = true;
    dec.send_packet(&pkt).unwrap();
    assert!(
        dec.parity_errors() > 0,
        "byte-shifted stream produced no parity errors"
    );
}
