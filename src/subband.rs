//! Per-(channel, subband) ADPCM state machine for aptX.
//!
//! Recovered structure (trace doc §4.3):
//!
//! - Backward-adaptive Jayant-style quantizer with a per-subband
//!   `factor_select` index (capped) and a derived `quantization_factor`
//!   pulled from the common 32-entry `QUANTIZATION_FACTORS` table.
//! - Two-piece predictor:
//!   - Short-term IIR over the previous reconstructed sample
//!     (2 weights `s_weight[2]`).
//!   - Long-term predictor of order N over the reconstructed-difference
//!     history (sign-LMS update on `d_weight[N]`, history depth `2N`).
//! - The decoder runs **the same arithmetic** as the encoder using only
//!   the codeword + previous state, which is what keeps them in
//!   bit-identical lock-step.
//!
//! ## Clean-room placeholder warning
//!
//! As with `tables.rs`, the **specific update-rule constants** here are
//! placeholders chosen for self-roundtrip stability. The real
//! Qualcomm rules are NDA — see README §Compatibility.

use crate::tables::{Subband, SubbandTables, Variant, QUANTIZATION_FACTORS};

#[derive(Clone, Debug)]
pub struct SubbandState {
    pub variant: Variant,
    pub subband: Subband,
    pub tables: SubbandTables,

    /// Index into [`QUANTIZATION_FACTORS`].
    pub factor_select: i32,
    /// Most recent codeword's reconstructed difference (signed).
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

impl SubbandState {
    pub fn new(variant: Variant, subband: Subband) -> Self {
        let n = subband.prediction_order();
        Self {
            variant,
            subband,
            tables: SubbandTables::new(variant, subband),
            factor_select: 0,
            reconstructed_difference: 0,
            s_weight: [0, 0],
            d_weight: vec![0; n],
            reconstructed_differences: vec![0; 2 * n],
            previous_reconstructed_sample: 0,
        }
    }

    pub fn quantization_factor(&self) -> i32 {
        let idx = self
            .factor_select
            .clamp(0, (QUANTIZATION_FACTORS.len() - 1) as i32) as usize;
        QUANTIZATION_FACTORS[idx]
    }

    /// Forward predict: returns the predicted sample for the next block.
    /// Uses `s_weight` and `d_weight`/`reconstructed_differences`.
    pub fn predict(&self) -> i32 {
        // Short-term contribution: (s_weight[0] + s_weight[1]) * prev_recon >> 15.
        let s_term = ((self.s_weight[0] as i64 + self.s_weight[1] as i64)
            * self.previous_reconstructed_sample as i64)
            >> 15;
        // Long-term contribution: dot product of d_weight with the most-
        // recent N entries of the reconstructed_differences history.
        let n = self.subband.prediction_order();
        let mut d_term: i64 = 0;
        for i in 0..n {
            // The history is a circular buffer of length 2N — we sum the
            // first N entries.
            d_term += self.d_weight[i] as i64 * self.reconstructed_differences[i] as i64;
        }
        d_term >>= 15;
        (s_term + d_term) as i32
    }

    /// Encoder-side: quantize a signed difference (subband_sample -
    /// predicted_sample) into a signed codeword. Returns the codeword
    /// in the *signed* range `[-2^(bits-1), 2^(bits-1) - 1]` with the
    /// magnitude clamped to the table size, plus the per-sample
    /// quantization error (used by the parity-injection chooser).
    pub fn quantize(&self, diff: i32) -> (i32, i32) {
        let qf = self.quantization_factor().max(1);
        let bits = self.subband.bits(self.variant);
        let mag = diff.unsigned_abs() as i64;
        // Find the largest interval threshold (scaled by qf) that's <= mag.
        let mut idx: i32 = 0;
        let scale = qf as i64;
        for (i, &thr) in self.tables.intervals.iter().enumerate() {
            if (thr as i64) * scale <= mag {
                idx = i as i32;
            } else {
                break;
            }
        }
        // Cap at the maximum signed magnitude representable in `bits`.
        let max_mag = (1i32 << (bits - 1)) - 1;
        let idx = idx.min(max_mag);
        let codeword = if diff < 0 { -idx } else { idx };
        // Quantization error = how far diff is from the centre of the
        // chosen interval (scaled).
        let centre = (self.tables.intervals[idx as usize] as i64) * scale;
        let err = (mag - centre).unsigned_abs() as i32;
        (codeword, err)
    }

    /// Decoder-side: invert a signed codeword to the reconstructed
    /// difference. Includes the dither factor.
    pub fn invert_quantize(&self, codeword: i32, dither: i32) -> i32 {
        let qf = self.quantization_factor().max(1);
        let mag = codeword.unsigned_abs() as usize;
        let max_idx = self.tables.intervals.len() - 1;
        let idx = mag.min(max_idx);
        let interval = self.tables.intervals[idx] as i64;
        let dith = self.tables.dither_factors[idx] as i64 + dither as i64;
        // Reconstructed magnitude = interval * qf + dither_contrib.
        let recon_mag = interval * qf as i64 + (dith >> 4);
        if codeword < 0 {
            -(recon_mag as i32)
        } else {
            recon_mag as i32
        }
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
        // Step-size adaptation: shift factor_select by the table's
        // per-interval offset, clamped.
        let mag = codeword.unsigned_abs() as usize;
        let max_idx = self.tables.factor_select_offsets.len() - 1;
        let idx = mag.min(max_idx);
        let offset = self.tables.factor_select_offsets[idx];
        // Apply 7/8 leak so the adaptation decays in absence of new info
        // (textbook Jayant rule with a leakage factor).
        self.factor_select = ((self.factor_select * 7) / 8) + offset;
        let cap = self.subband.factor_max();
        self.factor_select = self.factor_select.clamp(0, cap);

        // Predictor update: shift the difference history one slot, drop
        // the latest reconstructed_diff in front.
        self.reconstructed_differences.rotate_right(1);
        self.reconstructed_differences[0] = reconstructed_diff;

        // Sign-LMS update on d_weight: nudge each weight by the sign of
        // (codeword × past_reconstructed_diff), small magnitude.
        let n = self.subband.prediction_order();
        let csign = codeword.signum();
        for i in 0..n {
            let psign = self.reconstructed_differences[i + 1.min(2 * n - 1)].signum();
            // Tiny nudge — keep weights in a small range.
            let nudge = csign * psign;
            self.d_weight[i] = (self.d_weight[i] + nudge).clamp(-32_768, 32_767);
            // Decay term to keep the predictor stable.
            self.d_weight[i] = (self.d_weight[i] * 255) / 256;
        }

        // Short-term weight update — a similar small nudge plus decay.
        let ssign = codeword.signum() * reconstructed_sample.signum();
        self.s_weight[0] = ((self.s_weight[0] + ssign) * 255 / 256).clamp(-32_768, 32_767);
        self.s_weight[1] = ((self.s_weight[1] + ssign / 2) * 255 / 256).clamp(-32_768, 32_767);

        // Anchor for the next prediction.
        self.previous_reconstructed_sample = reconstructed_sample;
        self.reconstructed_difference = reconstructed_diff;
    }
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
        // Push a stream of full-magnitude codewords; the largest interval
        // index has a positive offset, so factor_select must climb to the
        // cap.
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
}
