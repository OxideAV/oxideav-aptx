//! Codeword-history-driven dither generator (trace doc §4.4).
//!
//! Per channel, the codec maintains a 32-bit shift register seeded by
//! selected bits of the previous block's LF, MLF and MHF codewords (HF
//! is **excluded** because its low bit is reserved for parity sync).
//! Every block this register produces:
//!
//! 1. Four per-subband 32-bit fractional dither values (one per band).
//! 2. A 1-bit `dither_parity` that participates in the 8-block parity
//!    sync invariant (§5.3).
//!
//! Encoder and decoder use the **same** dither, so the dither does
//! not need to be transmitted — it's recomputed from the past
//! codeword stream on both sides.
//!
//! ## Clean-room placeholder warning
//!
//! The exact bit-mixing function used by Qualcomm is NDA. This module
//! ships a structurally-equivalent LFSR-derived placeholder: a 32-bit
//! Fibonacci LFSR seeded from the codeword history and clocked once
//! per dither output. Encoder and decoder must use the same one to
//! stay in lock-step; switching to the real Qualcomm rule is a
//! drop-in replacement.

#[derive(Clone, Debug, Default)]
pub struct DitherGen {
    /// 32-bit shift register holding the recent codeword history.
    /// Selected bits feed the LFSR seed each block.
    history: u32,
    /// Last dither_parity bit emitted.
    parity: u8,
}

impl DitherGen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mix the previous block's codewords (LF, MLF, MHF only — HF is
    /// excluded) into the history register.
    pub fn ingest(&mut self, lf: i32, mlf: i32, mhf: i32) {
        // Pack the low bits of each into the history. We rotate the
        // history left by 6 to make room for the 6-ish bits we drop in.
        let mix =
            ((lf as u32) & 0x7F) ^ (((mlf as u32) & 0x0F) << 7) ^ (((mhf as u32) & 0x03) << 11);
        self.history = self.history.rotate_left(13) ^ mix.wrapping_mul(0x9E37_79B1);
    }

    /// Produce 4 per-subband dither values (signed) plus the 1-bit
    /// parity for this block. The values are scaled to a small signed
    /// range; the subband ADPCM scales them further by its own
    /// `quantization_factor`.
    pub fn next_block(&mut self) -> ([i32; 4], u8) {
        let mut state = self.history;
        let mut out = [0i32; 4];
        for v in &mut out {
            // 32-bit Fibonacci LFSR taps at 32, 22, 2, 1.
            let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ state) & 1;
            state = (state << 1) | bit;
            // Take the upper 16 bits as a signed dither in roughly [-32k, +32k).
            *v = (state as i32) >> 16;
        }
        self.history = state;
        self.parity = (state & 1) as u8;
        (out, self.parity)
    }

    pub fn parity(&self) -> u8 {
        self.parity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_when_history_matches() {
        let mut a = DitherGen::new();
        let mut b = DitherGen::new();
        a.ingest(5, 1, 0);
        b.ingest(5, 1, 0);
        let (va, pa) = a.next_block();
        let (vb, pb) = b.next_block();
        assert_eq!(va, vb);
        assert_eq!(pa, pb);
    }

    #[test]
    fn distinct_history_yields_distinct_dither() {
        let mut a = DitherGen::new();
        let mut b = DitherGen::new();
        a.ingest(5, 1, 0);
        b.ingest(7, 1, 0);
        let (va, _) = a.next_block();
        let (vb, _) = b.next_block();
        assert_ne!(va, vb);
    }
}
