//! Per-(channel, subband) ADPCM state machine for aptX.
//!
//! Recovered structure (trace doc §4.3):
//!
//! - Backward-adaptive Jayant-style quantizer with a per-subband
//!   `factor_select` index (capped at the subband's `factor_max`) and
//!   a derived `quantization_factor` pulled from the common 32-entry
//!   `QUANTIZATION_FACTORS` table via the index/shift split documented
//!   in `data/aptx-quantizer-tables.md`.
//! - Two-piece predictor:
//!   - Short-term IIR over the previous reconstructed sample
//!     (2 weights `s_weight[2]`).
//!   - Long-term predictor of order N over the reconstructed-difference
//!     history (sign-LMS update on `d_weight[N]`, history depth `2N`).
//! - The decoder runs **the same arithmetic** as the encoder using only
//!   the codeword + previous state, which is what keeps them in
//!   bit-identical lock-step.
//!
//! All numerical constants here come from
//! `docs/audio/aptx/data/aptx-quantizer-tables.md` and the trace doc;
//! no NDA-only Qualcomm material is consulted.

use crate::tables::{Subband, SubbandTables, Variant, QUANTIZATION_FACTORS};

/// Leak coefficient for the `factor_select` integrator
/// (`32620 / 32768 ≈ 0.99548`, ~0.45 % decay per block).
const FACTOR_SELECT_LEAK: i64 = 32620;

#[derive(Clone, Debug)]
pub struct SubbandState {
    pub variant: Variant,
    pub subband: Subband,
    pub tables: SubbandTables,

    /// Step-size index into the quantizer ladder. Capped at
    /// `subband.factor_max()`.
    pub factor_select: i32,
    /// Cached current quantization factor (recomputed on each
    /// `update_state`).
    pub quantization_factor: i32,
    /// Most recent reconstructed difference (signed, post-dither).
    pub reconstructed_difference: i32,

    /// Two short-term sample-domain predictor weights (Q15-scaled).
    pub s_weight: [i32; 2],
    /// Long-term difference-domain weights, length = prediction_order().
    pub d_weight: Vec<i32>,
    /// Reconstructed-difference history, length = 2 × prediction_order().
    pub reconstructed_differences: Vec<i32>,
    /// Previous reconstructed sample — anchor for the IIR sample predictor.
    pub previous_reconstructed_sample: i32,
}

/// Compute the quantization factor from `factor_select`, per the spec
/// sidecar §"Quantizer-update rule":
///
/// `step_idx = (factor_select & 0xFF) >> 3`
/// `shift    = (factor_max  − factor_select) >> 8`
/// `qf       = (QUANTIZATION_FACTORS[step_idx] << 11) >> shift`
fn compute_quantization_factor(factor_select: i32, factor_max: i32) -> i32 {
    let fs = factor_select.clamp(0, factor_max);
    let step_idx = ((fs & 0xFF) >> 3) as usize;
    let shift = ((factor_max - fs) >> 8).max(0) as u32;
    let qf = (QUANTIZATION_FACTORS[step_idx] as i64) << 11;
    (qf >> shift) as i32
}

impl SubbandState {
    pub fn new(variant: Variant, subband: Subband) -> Self {
        let n = subband.prediction_order();
        let factor_max = subband.factor_max();
        Self {
            variant,
            subband,
            tables: SubbandTables::new(variant, subband),
            factor_select: 0,
            quantization_factor: compute_quantization_factor(0, factor_max),
            reconstructed_difference: 0,
            s_weight: [0, 0],
            d_weight: vec![0; n],
            reconstructed_differences: vec![0; 2 * n],
            previous_reconstructed_sample: 0,
        }
    }

    pub fn quantization_factor(&self) -> i32 {
        self.quantization_factor
    }

    /// Forward predict: returns the predicted sample for the next block.
    /// Uses `s_weight` and `d_weight` / `reconstructed_differences`.
    pub fn predict(&self) -> i32 {
        // Short-term contribution: (s_weight[0] + s_weight[1]) * prev_recon >> 22.
        let s_term = ((self.s_weight[0] as i64 + self.s_weight[1] as i64)
            * self.previous_reconstructed_sample as i64)
            >> 22;
        // Long-term contribution: dot product of d_weight with the most-
        // recent N entries of the reconstructed_differences history.
        let n = self.subband.prediction_order();
        let mut d_term: i64 = 0;
        for i in 0..n {
            d_term += self.d_weight[i] as i64 * self.reconstructed_differences[i] as i64;
        }
        d_term >>= 22;
        clip24((s_term + d_term) as i32)
    }

    /// Encoder-side: quantize a signed difference (`subband_sample -
    /// predicted_sample`) into a signed codeword. Returns the codeword
    /// in the *signed* range `[-2^(bits-1), 2^(bits-1) - 1]` plus the
    /// per-sample quantization error (used by the parity-injection
    /// chooser).
    pub fn quantize(&self, diff: i32) -> (i32, i32) {
        let qf = self.quantization_factor.max(1) as i64;
        let bits = self.subband.bits(self.variant);
        let max_mag = (1i32 << (bits - 1)) - 1;
        // The decoder computes recon_diff = (qf * qr) >> 19 where qr
        // is the (signed) dither-blended interval midpoint. The
        // inverse relationship for the encoder: find the largest
        // index i such that intervals[i] * qf <= |diff| << 19.
        let scaled_diff = (diff.unsigned_abs() as i64) << 19;
        // Binary search on the positive part of the table (skip index 0
        // which is a negative sentinel).
        let intervals = self.tables.intervals;
        // Find largest i in [1, intervals.len()-1] with intervals[i]*qf <= scaled_diff.
        let mut lo = 1usize;
        let mut hi = intervals.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let thr = (intervals[mid] as i64) * qf;
            if thr <= scaled_diff {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // `lo` now points one past the largest matching index.
        let idx = (lo as i32 - 1).max(0).min(max_mag);
        let codeword = if diff < 0 { -idx } else { idx };
        // Quantization error: distance to the chosen interval boundary.
        let centre = (intervals[idx as usize] as i64) * qf;
        let err = (scaled_diff - centre).unsigned_abs() as i32;
        (codeword, err)
    }

    /// Decoder-side: invert a signed codeword to the reconstructed
    /// difference. Folds in the dither factor (which the encoder
    /// computes the same way, so encoder and decoder stay in
    /// lock-step).
    ///
    /// Per the spec sidecar:
    /// `reconstructed_difference = (quantization_factor * qr) >> 19`
    /// where `qr` is the dither-blended midpoint of the codeword's
    /// quantizer interval (sign-applied).
    pub fn invert_quantize(&self, codeword: i32, dither: i32) -> i32 {
        let qf = self.quantization_factor.max(1) as i64;
        let mag = codeword.unsigned_abs() as usize;
        let max_idx = self.tables.intervals.len() - 1;
        let idx = mag.min(max_idx);
        // The interval magnitude is intervals[idx] for idx >= 1; for
        // idx 0 (codeword 0) we treat the magnitude as 0 (the negative
        // sentinel at intervals[0] is only consulted by the encoder's
        // search lower-bound).
        let interval_mag = if idx == 0 {
            0i64
        } else {
            self.tables.intervals[idx] as i64
        };
        let invert_dith = self.tables.invert_quantize_dither_factors[idx] as i64;
        // Blend the dither into the interval magnitude. Compute the
        // unsigned-magnitude path then sign-apply at the end so the
        // result is exactly negate-symmetric in `codeword`.
        let dith_blend = (dither.unsigned_abs() as i64 * invert_dith) >> 23;
        let qr_mag = interval_mag + dith_blend;
        let abs_recon = (qf * qr_mag) >> 19;
        let result = if codeword < 0 { -abs_recon } else { abs_recon };
        result as i32
    }

    /// Update the predictor / step-size state from the codeword and
    /// reconstructed sample. Called identically by encoder and decoder
    /// after each codeword is emitted/received.
    pub fn update_state(
        &mut self,
        codeword: i32,
        reconstructed_diff: i32,
        reconstructed_sample: i32,
    ) {
        // ----- Step-size adaptation -----
        // factor_select_new = clip(round((LEAK * factor_select +
        //   (offset << 15)) >> 15), 0, factor_max)
        let mag = codeword.unsigned_abs() as usize;
        let max_idx = self.tables.factor_select_offsets.len() - 1;
        let idx = mag.min(max_idx);
        let offset = self.tables.factor_select_offsets[idx] as i64;
        let factor_max = self.subband.factor_max();
        let acc = FACTOR_SELECT_LEAK * (self.factor_select as i64) + (offset << 15);
        // Round-shift right by 15.
        let new_fs = ((acc + (1 << 14)) >> 15) as i32;
        self.factor_select = new_fs.clamp(0, factor_max);
        // Recompute cached quantization factor.
        self.quantization_factor = compute_quantization_factor(self.factor_select, factor_max);

        // ----- Predictor history update -----
        self.reconstructed_differences.rotate_right(1);
        self.reconstructed_differences[0] = reconstructed_diff;

        // ----- Sign-LMS d_weight update -----
        // Each weight is pulled toward `±2^23` (the sign of the current
        // reconstructed difference, multiplied by the sign of the i-th
        // past reconstructed difference) by a single-pole filter with
        // leak `255/256`.
        let n = self.subband.prediction_order();
        let csign = reconstructed_diff.signum() as i64;
        for i in 0..n {
            let psign = self.reconstructed_differences[i + 1.min(2 * n - 1)].signum() as i64;
            let target = csign * psign * (1i64 << 23);
            let leak = (self.d_weight[i] as i64 - target) >> 8;
            let updated = self.d_weight[i] as i64 - leak;
            self.d_weight[i] = updated.clamp(-(1i64 << 23), (1i64 << 23) - 1) as i32;
        }

        // ----- Short-term s_weight update -----
        // Per the trace doc §4.4:
        //   s_weight[0]: leak 254/256, input gain 0x800000 per
        //     matching-sign indicator, clipped to ±0x300000.
        //   s_weight[1]: leak 255/256, input gain 0xC00000, clipped to
        //     ±(0x3C0000 − s_weight[0]) — bounded by the headroom
        //     remaining after the first short-term tap.
        let s_sign = if reconstructed_sample.signum() == self.previous_reconstructed_sample.signum()
            && reconstructed_sample != 0
        {
            1i64
        } else if reconstructed_sample != 0 {
            -1i64
        } else {
            0
        };
        let target0 = s_sign * 0x800000i64;
        // leak factor 254/256 = (1 - 2/256) → effective shift-by-7
        // when applied as `weight -= (weight - target) >> 7`. Use the
        // doc-stated 254/256 form via a manual leaky update.
        let s0 = self.s_weight[0] as i64;
        let s0_new = s0 - (((s0 - target0) * 2) >> 8);
        self.s_weight[0] = s0_new.clamp(-0x300000, 0x300000) as i32;
        let target1 = s_sign * 0xC00000i64;
        let s1 = self.s_weight[1] as i64;
        let s1_new = s1 - ((s1 - target1) >> 8);
        let s0_abs = self.s_weight[0].unsigned_abs() as i64;
        let s1_cap = (0x3C0000i64 - s0_abs).max(0);
        self.s_weight[1] = s1_new.clamp(-s1_cap, s1_cap) as i32;

        // Anchor for next prediction.
        self.previous_reconstructed_sample = reconstructed_sample;
        self.reconstructed_difference = reconstructed_diff;
    }
}

/// 24-bit signed clip.
fn clip24(v: i32) -> i32 {
    const MAX: i32 = (1 << 23) - 1;
    const MIN: i32 = -(1 << 23);
    v.clamp(MIN, MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_predicts_zero() {
        let s = SubbandState::new(Variant::Classic, Subband::Lf);
        assert_eq!(s.predict(), 0);
    }

    #[test]
    fn quantize_zero_difference_is_zero() {
        let s = SubbandState::new(Variant::Classic, Subband::Lf);
        let (cw, _err) = s.quantize(0);
        assert_eq!(cw, 0);
    }

    #[test]
    fn invert_quantize_negates_for_negative_codeword() {
        let s = SubbandState::new(Variant::Classic, Subband::Mlf);
        let pos = s.invert_quantize(3, 0);
        let neg = s.invert_quantize(-3, 0);
        assert_eq!(pos, -neg);
    }

    #[test]
    fn factor_select_increments_under_persistent_signal() {
        let mut s = SubbandState::new(Variant::Classic, Subband::Lf);
        // Push a stream of large-magnitude codewords; the largest
        // interval index has a large positive offset (522), so
        // factor_select must climb from 0 toward factor_max.
        let cw = 60; // near 7-bit signed max (63)
        for _ in 0..200 {
            let recon = s.invert_quantize(cw, 0);
            s.update_state(cw, recon, recon);
        }
        assert!(
            s.factor_select > 0,
            "factor_select did not adapt: {}",
            s.factor_select
        );
    }

    #[test]
    fn classic_codeword_fits_in_band_bits() {
        for sb in [Subband::Lf, Subband::Mlf, Subband::Mhf, Subband::Hf] {
            let s = SubbandState::new(Variant::Classic, sb);
            let bits = sb.bits(Variant::Classic);
            let (cw, _) = s.quantize(i32::MAX / 2);
            let max = (1i32 << (bits - 1)) - 1;
            assert!(
                cw.abs() <= max,
                "{sb:?}: codeword {cw} exceeds {bits}-bit signed range ±{max}"
            );
        }
    }

    #[test]
    fn factor_select_capped_at_factor_max() {
        // Push huge offsets and verify the cap holds.
        let mut s = SubbandState::new(Variant::Classic, Subband::Lf);
        let cw = 60;
        for _ in 0..10_000 {
            let recon = s.invert_quantize(cw, 0);
            s.update_state(cw, recon, recon);
        }
        assert!(s.factor_select <= Subband::Lf.factor_max());
    }

    #[test]
    fn subband_self_roundtrip_tracks_input() {
        // Drive a single subband ADPCM with a slow signal and check
        // that the encoder→decoder reconstruction is in lock-step and
        // tracks the input within ~30 dB after the convergence
        // transient (~20 blocks).
        let mut enc = SubbandState::new(Variant::Classic, Subband::Lf);
        let mut dec = SubbandState::new(Variant::Classic, Subband::Lf);
        let mut sum_sig = 0.0f64;
        let mut sum_err = 0.0f64;
        for n in 0..200 {
            let target = ((n as f64 * 0.05).sin() * 1_000_000.0) as i32;
            let pred = enc.predict();
            let diff = target - pred;
            let (cw, _err) = enc.quantize(diff);
            let recon_diff = enc.invert_quantize(cw, 0);
            let recon = pred + recon_diff;
            enc.update_state(cw, recon_diff, recon);
            let pred_d = dec.predict();
            let recon_diff_d = dec.invert_quantize(cw, 0);
            let recon_d = pred_d + recon_diff_d;
            dec.update_state(cw, recon_diff_d, recon_d);
            // Encoder and decoder must stay in bit-identical lock-step.
            assert_eq!(recon, recon_d, "encoder/decoder desync at n={n}");
            if n >= 30 {
                let e = (target - recon_d) as f64;
                sum_err += e * e;
                sum_sig += (target as f64).powi(2);
            }
        }
        let psnr = 10.0 * (sum_sig / sum_err.max(1.0)).log10();
        assert!(
            psnr > 30.0,
            "subband ADPCM tracking PSNR too low: {psnr:.2} dB"
        );
    }

    #[test]
    fn quantization_factor_at_zero_factor_select_is_known() {
        // factor_select=0 → step_idx=0 → QUANTIZATION_FACTORS[0]=2048.
        // shift = factor_max >> 8.
        let qf = compute_quantization_factor(0, 0x11FF);
        // (2048 << 11) >> 0x11 = 4194304 >> 17 = 32.
        assert_eq!(qf, 32);
    }
}
