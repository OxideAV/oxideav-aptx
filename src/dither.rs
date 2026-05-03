//! Codeword-history-driven dither generator (trace doc §4.4-4.5).
//!
//! Per channel, the codec maintains a 32-bit shift register seeded by
//! selected bits of the previous block's LF, MLF and MHF codewords (HF
//! is **excluded** because its low bit is reserved for parity sync).
//! Every block this register produces:
//!
//! 1. Four per-subband signed dither values (one per band).
//! 2. A 1-bit `dither_parity` that participates in the 8-block parity
//!    sync invariant (§5.3).
//!
//! Encoder and decoder use the **same** dither, so the dither does
//! not need to be transmitted — it's recomputed from the past
//! codeword stream on both sides.
//!
//! ## Status
//!
//! The codeword-history *update* equation in this module follows the
//! trace-doc rule (shift-by-4 plus a 4-bit field built from selected
//! bits of LF / MLF / MHF), but the *output* mapping from
//! `codeword_history` to per-subband dither values uses an LFSR-style
//! mixer rather than the spec's `× 5_184_443` post-shift fractional
//! decomposition. This produces a structurally consistent encoder ↔
//! decoder dither (both sides compute the same values from the same
//! history) but the per-block dither numbers are not bit-identical
//! with FFmpeg's `aptx`/`aptx_hd`. Replacing this output mapping with
//! the spec's exact bit-position decomposition is a follow-on
//! refinement.

#[derive(Clone, Debug, Default)]
pub struct DitherGen {
    /// 32-bit shift register holding the recent codeword history.
    history: u32,
    /// Last `dither_parity` bit emitted.
    parity: u8,
}

impl DitherGen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mix the previous block's codewords (LF, MLF, MHF only — HF is
    /// excluded) into the codeword-history register. Per the trace doc
    /// §4.5: the register is shifted left 4 bits and ORed with a 4-bit
    /// field built from `LF[0]`, `LF[1]`, `MLF[1]`, `MHF[0]`.
    pub fn ingest(&mut self, lf: i32, mlf: i32, mhf: i32) {
        let field: u32 = ((lf as u32) & 0x1)
            | (((lf as u32) >> 1) & 0x1) << 1
            | (((mlf as u32) >> 1) & 0x1) << 2
            | ((mhf as u32) & 0x1) << 3;
        self.history = (self.history << 4) | field;
    }

    /// Produce 4 per-subband signed dither values plus the 1-bit
    /// parity for this block. Output mapping is an LFSR mixer over the
    /// codeword-history register; see module docstring.
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
        // Mix the post-LFSR state back into the history register so the
        // next block sees the updated state (this is what makes
        // distinct codeword histories produce distinct dithers).
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
        // Drive several blocks of distinct codewords so the
        // codeword_history register has bits in the upper byte
        // (otherwise the (state >> 16) sign-extend is identically zero
        // for both gens).
        for _ in 0..10 {
            a.ingest(0xF, 0xF, 0xF);
            b.ingest(0, 0, 0);
        }
        let (va, _) = a.next_block();
        let (vb, _) = b.next_block();
        assert_ne!(va, vb);
    }

    #[test]
    fn ingest_packs_correct_bits_per_spec() {
        // After ingesting LF=0b11, MLF=0b10, MHF=0b1, the field should
        // be: bit0=LF[0]=1, bit1=LF[1]=1, bit2=MLF[1]=1, bit3=MHF[0]=1
        // → 0b1111 = 0xF.
        let mut g = DitherGen::new();
        g.ingest(0b11, 0b10, 0b1);
        assert_eq!(g.history, 0xF);
    }
}
