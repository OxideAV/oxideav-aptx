//! aptX encoder (classic and HD).
//!
//! Per stereo block (4 PCM samples per channel):
//!
//! 1. Run each channel's PCM through QMF analysis → 4 subband samples.
//! 2. Generate per-channel dither from the codeword history.
//! 3. Per subband: subtract predicted_sample, quantize the difference,
//!    emit a signed codeword.
//! 4. Compute parity over all 8 codewords ⊕ both dither parities;
//!    if it doesn't match the required value for the current
//!    `sync_idx` (zero for blocks 0..6, one for block 7), nudge the
//!    smallest-error codeword by ±1.
//! 5. Update each subband's predictor / step-size state.
//! 6. Pack 4 codewords per channel into a 16-bit (classic) or 24-bit
//!    (HD) big-endian word; emit two of those per block.

use std::collections::VecDeque;

use oxideav_core::Encoder;
use oxideav_core::{
    CodecId, CodecParameters, Error, Frame, MediaType, Packet, Result, SampleFormat, TimeBase,
};

use crate::channel::EncoderChannel;
use crate::codeword::{pack_block, pack_channel};
use crate::tables::{Subband, Variant};

pub const SAMPLE_RATES: &[u32] = &[8_000, 16_000, 24_000, 32_000, 44_100, 48_000];

pub fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    let sample_rate = params.sample_rate.unwrap_or(44_100);
    if !SAMPLE_RATES.contains(&sample_rate) {
        return Err(Error::unsupported(format!(
            "aptX encoder: sample rate {sample_rate} not in {SAMPLE_RATES:?}"
        )));
    }
    let channels = params.channels.unwrap_or(2);
    if channels != 2 {
        return Err(Error::unsupported(format!(
            "aptX encoder: stereo only (got {channels} channels)"
        )));
    }
    let sample_format = params.sample_format.unwrap_or(SampleFormat::S16);
    if sample_format != SampleFormat::S16 {
        return Err(Error::unsupported(format!(
            "aptX encoder: input sample format {sample_format:?} not supported (need S16)"
        )));
    }
    let variant = variant_from_codec_id(&params.codec_id)?;

    let mut output = params.clone();
    output.media_type = MediaType::Audio;
    output.sample_format = Some(SampleFormat::S16);
    output.channels = Some(2);
    output.sample_rate = Some(sample_rate);
    output.bit_rate = Some(stream_bit_rate(variant, sample_rate));
    Ok(Box::new(AptxEncoder::new(output, variant)))
}

fn variant_from_codec_id(id: &CodecId) -> Result<Variant> {
    match id.as_str() {
        "aptx" => Ok(Variant::Classic),
        "aptx_hd" => Ok(Variant::Hd),
        other => Err(Error::unsupported(format!(
            "aptX encoder: unknown codec id {other:?}"
        ))),
    }
}

fn stream_bit_rate(variant: Variant, sample_rate: u32) -> u64 {
    // Classic: 4 B per group of 4 PCM samples per channel × 2 channels →
    // 4 B every 4 PCM frames at the input rate. So bps = sample_rate × bytes / 4 × 8.
    // For 44.1k classic: 44100 × 4 / 4 × 8 = 352800.
    let bytes_per_block = variant.block_bytes() as u64;
    (sample_rate as u64) * bytes_per_block * 8 / 4
}

pub struct AptxEncoder {
    output_params: CodecParameters,
    time_base: TimeBase,
    variant: Variant,
    channels: [EncoderChannel; 2],
    sync_idx: u8,
    /// PCM carry-over (we need 4-sample groups per channel; samples are
    /// interleaved s16 so 8 PCM samples per group). Carry interleaved
    /// samples until we have a full block.
    carry: Vec<i16>,
    pending: VecDeque<Packet>,
    next_pts: i64,
    /// Accumulated encoded bytes within a single send_frame call's
    /// emitted packet.
    enc_buf: Vec<u8>,
}

impl AptxEncoder {
    pub fn new(output_params: CodecParameters, variant: Variant) -> Self {
        let sr = output_params.sample_rate.unwrap_or(44_100) as i64;
        Self {
            time_base: TimeBase::new(1, sr),
            output_params,
            variant,
            channels: [EncoderChannel::new(variant), EncoderChannel::new(variant)],
            sync_idx: 0,
            carry: Vec::with_capacity(8),
            pending: VecDeque::new(),
            next_pts: 0,
            enc_buf: Vec::new(),
        }
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    /// Encode exactly one stereo block (4 PCM samples × 2 channels = 8
    /// interleaved s16 samples in `pcm`). Pushes the resulting bytes
    /// onto `self.enc_buf`.
    fn encode_block(&mut self, pcm: &[i16]) {
        debug_assert_eq!(pcm.len(), 8);
        let mut left = [0i32; 4];
        let mut right = [0i32; 4];
        for i in 0..4 {
            // Promote to "24-bit effective" by shifting up by 8 — the trace
            // doc says FFmpeg does samples >> 8 internally; we go the other
            // way so the QMF sees the same dynamic range we'll produce on
            // decode.
            left[i] = (pcm[2 * i] as i32) << 8;
            right[i] = (pcm[2 * i + 1] as i32) << 8;
        }

        // QMF analysis per channel.
        let sb_left = self.channels[0].qmf.process(left);
        let sb_right = self.channels[1].qmf.process(right);

        // Dither per channel.
        let (dith_left, parity_left) = self.channels[0].dither.next_block();
        let (dith_right, parity_right) = self.channels[1].dither.next_block();

        // Per subband: predict, quantize.
        let mut codewords = [[0i32; 4]; 2];
        let mut errors = [[0i32; 4]; 2];
        let sb_samples = [sb_left, sb_right];
        for (ch_idx, ch) in self.channels.iter_mut().enumerate() {
            for band_idx in 0..4 {
                let predicted = ch.bands[band_idx].predict();
                let diff = sb_samples[ch_idx][band_idx] - predicted;
                let (cw, err) = ch.bands[band_idx].quantize(diff);
                codewords[ch_idx][band_idx] = cw;
                errors[ch_idx][band_idx] = err;
            }
        }

        // Parity-injection: compute current parity, decide if it matches
        // the required value for sync_idx.
        let required_parity: u8 = if self.sync_idx == 7 { 1 } else { 0 };
        let actual_parity = block_parity(&codewords, parity_left, parity_right);
        if actual_parity != required_parity {
            // Find the (channel, band) with the smallest error and flip
            // the LSB of its codeword by ±1.
            let mut best_ch = 0usize;
            let mut best_band = 0usize;
            let mut best_err = i32::MAX;
            for ch_idx in 0..2 {
                for band_idx in 0..4 {
                    if errors[ch_idx][band_idx] < best_err {
                        best_err = errors[ch_idx][band_idx];
                        best_ch = ch_idx;
                        best_band = band_idx;
                    }
                }
            }
            // Step ±1 — the direction doesn't really matter for parity, but
            // we pick +1 if the codeword is already <= 0, else -1, to stay
            // closer to the band's representable range.
            let cw = codewords[best_ch][best_band];
            codewords[best_ch][best_band] = if cw <= 0 { cw + 1 } else { cw - 1 };
        }

        // Decode-side update: invert quantize, update state.
        let dith = [dith_left, dith_right];
        for ch_idx in 0..2 {
            for band_idx in 0..4 {
                let cw = codewords[ch_idx][band_idx];
                let predicted = self.channels[ch_idx].bands[band_idx].predict();
                let recon_diff = self.channels[ch_idx].bands[band_idx]
                    .invert_quantize(cw, dith[ch_idx][band_idx]);
                let recon_sample = predicted + recon_diff;
                self.channels[ch_idx].bands[band_idx].update_state(cw, recon_diff, recon_sample);
            }
        }

        // Ingest codewords back into each channel's dither history (LF,
        // MLF, MHF only — HF excluded per §4.4).
        self.channels[0]
            .dither
            .ingest(codewords[0][0], codewords[0][1], codewords[0][2]);
        self.channels[1]
            .dither
            .ingest(codewords[1][0], codewords[1][1], codewords[1][2]);

        // Pack into the 4-byte (classic) or 6-byte (HD) block.
        let left_packed = pack_channel(self.variant, codewords[0]);
        let right_packed = pack_channel(self.variant, codewords[1]);
        let bytes = pack_block(self.variant, left_packed, right_packed);
        self.enc_buf.extend_from_slice(&bytes);

        self.sync_idx = (self.sync_idx + 1) % 8;
    }
}

/// XOR of all 8 codeword LSBs ⊕ both per-channel dither parities.
fn block_parity(codewords: &[[i32; 4]; 2], dpl: u8, dpr: u8) -> u8 {
    let mut p: u8 = dpl ^ dpr;
    for ch in 0..2 {
        for b in 0..4 {
            p ^= (codewords[ch][b] & 1) as u8;
        }
    }
    p & 1
}

impl Encoder for AptxEncoder {
    fn codec_id(&self) -> &CodecId {
        &self.output_params.codec_id
    }

    fn output_params(&self) -> &CodecParameters {
        &self.output_params
    }

    fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        let af = match frame {
            Frame::Audio(a) => a,
            _ => return Err(Error::invalid("aptX encoder: audio frames only")),
        };
        let bytes = af
            .data
            .first()
            .ok_or_else(|| Error::invalid("aptX encoder: empty frame"))?;
        if bytes.len() % 2 != 0 {
            return Err(Error::invalid("aptX encoder: odd byte count"));
        }
        // Decode the interleaved S16LE plane back to i16, prepend any carry,
        // then process in groups of 8 (= 4 PCM frames stereo).
        let mut samples: Vec<i16> = std::mem::take(&mut self.carry);
        samples.reserve(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let n_full = samples.len() / 8;
        self.enc_buf.clear();
        for i in 0..n_full {
            let block = &samples[i * 8..(i + 1) * 8];
            self.encode_block(block);
        }
        // Stash the trailing partial group (< 8 samples = < 4 PCM frames).
        let consumed = n_full * 8;
        self.carry = samples[consumed..].to_vec();

        let n_in = consumed / 2; // PCM frames consumed (stereo pairs).
        if !self.enc_buf.is_empty() {
            let pts = af.pts.or(Some(self.next_pts));
            self.next_pts = pts.unwrap_or(self.next_pts) + n_in as i64;
            let mut pkt = Packet::new(0, self.time_base, std::mem::take(&mut self.enc_buf));
            pkt.pts = pts;
            pkt.dts = pts;
            pkt.duration = Some(n_in as i64);
            pkt.flags.keyframe = true;
            self.pending.push_back(pkt);
        }
        Ok(())
    }

    fn receive_packet(&mut self) -> Result<Packet> {
        self.pending.pop_front().ok_or(Error::NeedMore)
    }

    fn flush(&mut self) -> Result<()> {
        // Pad the residual carry up to one full block of zeros, encode it,
        // emit a final packet if anything came out.
        if !self.carry.is_empty() {
            let need = 8 - self.carry.len();
            self.carry.extend(std::iter::repeat(0i16).take(need));
            self.enc_buf.clear();
            // Take ownership of carry to satisfy borrow checker.
            let block: Vec<i16> = std::mem::take(&mut self.carry);
            self.encode_block(&block);
            let pts = Some(self.next_pts);
            self.next_pts += 4;
            if !self.enc_buf.is_empty() {
                let mut pkt = Packet::new(0, self.time_base, std::mem::take(&mut self.enc_buf));
                pkt.pts = pts;
                pkt.dts = pts;
                pkt.duration = Some(4);
                pkt.flags.keyframe = true;
                self.pending.push_back(pkt);
            }
        }
        Ok(())
    }
}

// silences unused-import warning while leaving Subband importable for
// future expansion (HD-specific bit allocation lives there).
#[allow(dead_code)]
fn _subband_module_used(_: Subband) {}
