//! aptX decoder (classic and HD).
//!
//! Per stereo block (4 / 6 bytes):
//!
//! 1. Read both big-endian channel codewords; slice into 4 per-subband
//!    signed integers.
//! 2. Generate per-channel dither from the codeword history.
//! 3. Per subband: predict, invert-quantize the codeword, reconstruct
//!    the subband sample, update predictor / step-size state.
//! 4. Compute and verify the 8-block parity invariant; raise
//!    `parity_err=1` (logged but not fatal — playback continues) on
//!    mismatch.
//! 5. QMF synthesis per channel: 4 subband samples → 4 PCM samples.
//! 6. Emit 8 interleaved S16 samples per block.

use std::collections::VecDeque;

use oxideav_core::Decoder;
use oxideav_core::{AudioFrame, CodecId, CodecParameters, Error, Frame, Packet, Result};

use crate::channel::DecoderChannel;
use crate::codeword::{unpack_block, unpack_channel};
use crate::tables::Variant;
use crate::CODEC_ID_STR;

pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    let sample_rate = params.sample_rate.unwrap_or(44_100);
    if !crate::encoder::SAMPLE_RATES.contains(&sample_rate) {
        return Err(Error::unsupported(format!(
            "aptX decoder: sample rate {sample_rate} not in {:?}",
            crate::encoder::SAMPLE_RATES
        )));
    }
    let channels = params.channels.unwrap_or(2);
    if channels != 2 {
        return Err(Error::unsupported(format!(
            "aptX decoder: stereo only (got {channels} channels)"
        )));
    }
    let variant = match params.codec_id.as_str() {
        "aptx" => Variant::Classic,
        "aptx_hd" => Variant::Hd,
        other => {
            return Err(Error::unsupported(format!(
                "aptX decoder: unknown codec id {other:?}"
            )))
        }
    };
    Ok(Box::new(AptxDecoder::new(variant, sample_rate)))
}

pub struct AptxDecoder {
    codec_id: CodecId,
    variant: Variant,
    sample_rate: u32,
    channels: [DecoderChannel; 2],
    sync_idx: u8,
    /// Carry-over input bytes when packets aren't block-aligned.
    carry: Vec<u8>,
    pending: VecDeque<Frame>,
    drained: bool,
    next_pts: i64,
    /// Count of parity errors observed (reset on `reset`). Useful for
    /// observability — non-zero means the input lost block alignment.
    parity_errors: u64,
}

impl AptxDecoder {
    pub fn new(variant: Variant, sample_rate: u32) -> Self {
        Self {
            codec_id: CodecId::new(CODEC_ID_STR),
            variant,
            sample_rate,
            channels: [DecoderChannel::new(variant), DecoderChannel::new(variant)],
            sync_idx: 0,
            carry: Vec::new(),
            pending: VecDeque::new(),
            drained: false,
            next_pts: 0,
            parity_errors: 0,
        }
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    pub fn parity_errors(&self) -> u64 {
        self.parity_errors
    }

    /// Decode exactly one block from `bytes` (4 or 6 bytes depending on
    /// variant) into 8 interleaved S16 samples appended to `out`.
    fn decode_block(&mut self, bytes: &[u8], out: &mut Vec<i16>) {
        let (left_packed, right_packed) = match unpack_block(self.variant, bytes) {
            Some(p) => p,
            None => return,
        };
        let cw_left = unpack_channel(self.variant, left_packed);
        let cw_right = unpack_channel(self.variant, right_packed);
        let codewords = [cw_left, cw_right];

        let (dith_left, parity_left) = self.channels[0].dither.next_block();
        let (dith_right, parity_right) = self.channels[1].dither.next_block();

        // Verify parity before mutating predictor state.
        let observed = block_parity(&codewords, parity_left, parity_right);
        let required: u8 = if self.sync_idx == 7 { 1 } else { 0 };
        if observed != required {
            self.parity_errors = self.parity_errors.saturating_add(1);
        }
        self.sync_idx = (self.sync_idx + 1) % 8;

        // Reconstruct each subband.
        let mut subbands = [[0i32; 4]; 2];
        let dith = [dith_left, dith_right];
        for ch_idx in 0..2 {
            for band_idx in 0..4 {
                let cw = codewords[ch_idx][band_idx];
                let predicted = self.channels[ch_idx].bands[band_idx].predict();
                let recon_diff = self.channels[ch_idx].bands[band_idx]
                    .invert_quantize(cw, dith[ch_idx][band_idx]);
                let recon_sample = predicted + recon_diff;
                subbands[ch_idx][band_idx] = recon_sample;
                self.channels[ch_idx].bands[band_idx].update_state(cw, recon_diff, recon_sample);
            }
        }

        // Ingest LF/MLF/MHF codewords back into the dither history (HF
        // excluded per §4.4).
        self.channels[0]
            .dither
            .ingest(codewords[0][0], codewords[0][1], codewords[0][2]);
        self.channels[1]
            .dither
            .ingest(codewords[1][0], codewords[1][1], codewords[1][2]);

        // QMF synthesis per channel.
        let pcm_left = self.channels[0].qmf.process(subbands[0]);
        let pcm_right = self.channels[1].qmf.process(subbands[1]);

        // Re-shift down by 8 (we shifted up at encode time) and clip to S16.
        for i in 0..4 {
            let l = (pcm_left[i] >> 8).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            let r = (pcm_right[i] >> 8).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            out.push(l);
            out.push(r);
        }
    }
}

fn block_parity(codewords: &[[i32; 4]; 2], dpl: u8, dpr: u8) -> u8 {
    let mut p: u8 = dpl ^ dpr;
    for ch in 0..2 {
        for b in 0..4 {
            p ^= (codewords[ch][b] & 1) as u8;
        }
    }
    p & 1
}

impl Decoder for AptxDecoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if packet.data.is_empty() {
            return Ok(());
        }
        // Pre-pend any carry from the previous packet, then walk through
        // in block-aligned chunks.
        let mut buf: Vec<u8> = std::mem::take(&mut self.carry);
        buf.extend_from_slice(&packet.data);
        let block_size = self.variant.block_bytes();
        let n_full = buf.len() / block_size;
        let mut decoded: Vec<i16> = Vec::with_capacity(n_full * 8);
        for i in 0..n_full {
            let s = i * block_size;
            let block = &buf[s..s + block_size];
            self.decode_block(block, &mut decoded);
        }
        // Stash the trailing partial block.
        let consumed = n_full * block_size;
        self.carry = buf[consumed..].to_vec();

        if !decoded.is_empty() {
            let n_samples = (decoded.len() / 2) as u32;
            let mut bytes = Vec::with_capacity(decoded.len() * 2);
            for s in &decoded {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            let pts = packet.pts.or(Some(self.next_pts));
            self.next_pts = pts.unwrap_or(self.next_pts) + n_samples as i64;
            self.pending.push_back(Frame::Audio(AudioFrame {
                samples: n_samples,
                pts,
                data: vec![bytes],
            }));
        }
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        if let Some(f) = self.pending.pop_front() {
            return Ok(f);
        }
        if self.drained {
            return Err(Error::Eof);
        }
        Err(Error::NeedMore)
    }

    fn flush(&mut self) -> Result<()> {
        self.drained = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.channels = [
            DecoderChannel::new(self.variant),
            DecoderChannel::new(self.variant),
        ];
        self.sync_idx = 0;
        self.carry.clear();
        self.pending.clear();
        self.drained = false;
        self.next_pts = 0;
        self.parity_errors = 0;
        // Suppress unused-field warning — sample_rate is consumed via
        // CodecParameters reflection by callers, but isn't otherwise read.
        let _ = self.sample_rate;
        Ok(())
    }
}
