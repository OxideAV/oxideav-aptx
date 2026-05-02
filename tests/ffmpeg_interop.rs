//! Integration test: decode a real ffmpeg-encoded aptX stream.
//!
//! This crate ships **clean-room placeholder** quantizer tables and
//! QMF coefficients, so bit-exact interop with ffmpeg is NOT
//! expected. What we verify here is that the decoder structurally
//! handles a real aptX byte stream end-to-end without panicking, and
//! we capture the actual PSNR for documentation. When the
//! Qualcomm-specified tables become available and are dropped into
//! `tables.rs` / `qmf.rs`, this test should start producing high
//! PSNR; its presence makes the upgrade visible.
//!
//! The test is gated on the `ffmpeg` binary and silently skips if
//! it's unavailable (so CI without the optional toolchain still
//! passes).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use oxideav_core::{CodecId, CodecParameters, Frame, Packet, SampleFormat, TimeBase};
#[allow(unused_imports)]
use oxideav_core::{Decoder, Encoder};

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build a tiny aptx stream with ffmpeg into a tempdir, return the
/// path to the encoded `.aptx` file plus the matching reference PCM.
fn make_aptx_fixture(freq: u32, sample_rate: u32, dur_sec: f32) -> Option<(PathBuf, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("oxideav_aptx_test_{}_{}", freq, sample_rate));
    fs::create_dir_all(&dir).ok()?;
    let wav = dir.join("ref.wav");
    let aptx = dir.join("out.aptx");
    // 1) Generate stereo PCM with lavfi sine.
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={freq}:sample_rate={sample_rate}:duration={dur_sec}"),
            "-ac",
            "2",
        ])
        .arg(&wav)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    // 2) Encode with aptx classic.
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostats", "-loglevel", "error", "-y", "-i"])
        .arg(&wav)
        .args(["-c:a", "aptx", "-f", "aptx"])
        .arg(&aptx)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    Some((aptx, wav))
}

fn read_wav_pcm_s16(path: &PathBuf) -> Vec<i16> {
    // Minimal WAV reader: skip 44-byte header, read interleaved s16le.
    let bytes = fs::read(path).expect("read wav");
    assert!(bytes.len() > 44, "wav too short");
    bytes[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[test]
fn decode_ffmpeg_aptx_classic_runs_without_panic() {
    if !ffmpeg_available() {
        eprintln!("ffmpeg not available — skipping");
        return;
    }
    let Some((aptx_path, wav_path)) = make_aptx_fixture(1000, 44_100, 0.5) else {
        eprintln!("ffmpeg fixture build failed — skipping");
        return;
    };
    let aptx_bytes = fs::read(&aptx_path).expect("read aptx");
    eprintln!(
        "ffmpeg aptx classic stream: {} bytes ({} blocks of 4 B)",
        aptx_bytes.len(),
        aptx_bytes.len() / 4
    );
    // First block must match the documented state-zero output (trace
    // doc §6: classic = 4b bf 4b bf).
    assert_eq!(
        &aptx_bytes[0..4],
        &[0x4b, 0xbf, 0x4b, 0xbf],
        "ffmpeg aptx classic stream's cold-start block doesn't match trace doc §6"
    );

    let mut params = CodecParameters::audio(CodecId::new(oxideav_aptx::CODEC_ID_CLASSIC));
    params.sample_rate = Some(44_100);
    params.channels = Some(2);
    params.sample_format = Some(SampleFormat::S16);
    let mut dec = oxideav_aptx::decoder::make_decoder(&params).expect("decoder");

    let pkt = Packet::new(0, TimeBase::new(1, 44_100), aptx_bytes);
    dec.send_packet(&pkt).expect("send_packet");
    let mut decoded: Vec<i16> = Vec::new();
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
    eprintln!(
        "decoded {} samples (expected ~{})",
        decoded.len(),
        aptx_bytes_to_pcm_count(22_052)
    );
    assert!(
        !decoded.is_empty(),
        "decoder produced 0 PCM samples for a non-empty stream"
    );

    // Best-shift PSNR against the reference WAV. We *expect* this to
    // be poor (placeholder tables); the assertion is just that we
    // produce output, not that it's accurate.
    let reference = read_wav_pcm_s16(&wav_path);
    let snr = best_psnr(&reference, &decoded);
    eprintln!("ffmpeg-encoded aptX vs oxideav-aptx decoder PSNR = {snr:.2} dB (placeholder tables — bit-exact interop gated on Qualcomm tables)");
    // Per task spec: aptX is lossy with low fixed bit rate; PSNR
    // ~30-40 dB is normal for matched implementations. With our
    // placeholder tables the value will be much lower; we still
    // assert the decoder produced *some* signal energy, not zero.
    assert!(
        decoded.iter().any(|&s| s != 0),
        "decoder output is all zeros — pipeline is not running"
    );
}

fn aptx_bytes_to_pcm_count(bytes: usize) -> usize {
    // 4 PCM frames stereo per 4-byte block = 8 interleaved s16
    // samples per block.
    (bytes / 4) * 8
}

fn best_psnr(reference: &[i16], decoded: &[i16]) -> f64 {
    let n = reference.len().min(decoded.len());
    let head = (n / 4).min(512);
    let mut best = f64::NEG_INFINITY;
    for delay in 0..256 {
        if head + delay >= n {
            continue;
        }
        let span = n - head - delay;
        let mut err = 0.0f64;
        let mut sig = 0.0f64;
        for i in 0..span {
            let x = reference[head + i] as f64;
            let y = decoded[head + delay + i] as f64;
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
