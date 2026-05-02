//! Two-stage dyadic QMF (Quadrature Mirror Filter) tree for aptX.
//!
//! Per the trace doc (§4.2), aptX splits each channel of 4 PCM samples
//! into 4 subband samples via two stages of polyphase QMF:
//!
//! - **Outer QMF** (first stage): splits 4 input samples into two
//!   intermediate sub-bands of 2 samples each.
//! - **Inner QMF** (second stage): splits each intermediate band into
//!   two final sub-bands of 1 sample each.
//!
//! Result per channel per block:
//!
//! | Index | Name | Approx band (44.1 kHz) |
//! |------:|------|------------------------|
//! |   0   | LF   | 0 – 5.5 kHz            |
//! |   1   | MLF  | 5.5 – 11 kHz           |
//! |   2   | MHF  | 11 – 16.5 kHz          |
//! |   3   | HF   | 16.5 – 22 kHz          |
//!
//! ## Clean-room coefficient note
//!
//! The trace doc describes the cascade as two stages of 16-tap
//! polyphase mirror filters, but the **specific coefficients** are
//! Qualcomm-NDA. To preserve the structural shape of the trace
//! (two-stage split + dyadic combine) without quoting any third-party
//! material, this implementation uses the **trivially invertible Haar
//! kernel** (a degenerate 2-tap QMF: low = (x0+x1)/2, high = (x0-x1)/2,
//! with synthesis x0 = low + high, x1 = low - high). This perfect
//! reconstruction up to integer rounding lets the rest of the codec
//! pipeline be exercised cleanly.
//!
//! Bit-exact interop with FFmpeg's `aptx` decoder requires the real
//! 16-tap Qualcomm coefficient sets — see README §Compatibility for
//! the upgrade path.

/// Single-stage Haar QMF analysis pair: 2 samples in → (low, high).
/// Perfectly invertible (up to LSB rounding) by the synthesis pair.
fn qmf_split(s0: i32, s1: i32) -> (i32, i32) {
    let lo = (s0 + s1) >> 1;
    let hi = (s0 - s1) >> 1;
    (lo, hi)
}

/// Inverse of `qmf_split`. (lo, hi) → (s0, s1).
fn qmf_join(lo: i32, hi: i32) -> (i32, i32) {
    let s0 = lo + hi;
    let s1 = lo - hi;
    (s0, s1)
}

/// Full QMF analysis tree: 4 PCM samples in, 4 subband samples out
/// in [LF, MLF, MHF, HF] order.
#[derive(Clone, Debug, Default)]
pub struct QmfAnalysis {
    // Haar QMF is stateless — no shift register needed. But we keep a
    // unit-struct shape so the API matches the future 16-tap upgrade.
    _s: (),
}

impl QmfAnalysis {
    pub fn new() -> Self {
        Self { _s: () }
    }

    /// Split 4 input samples into [LF, MLF, MHF, HF].
    pub fn process(&mut self, samples: [i32; 4]) -> [i32; 4] {
        // Outer stage: pair-wise sums and differences.
        let (lo0, hi0) = qmf_split(samples[0], samples[1]);
        let (lo1, hi1) = qmf_split(samples[2], samples[3]);
        // Inner stage on each intermediate.
        let (lf, mlf) = qmf_split(lo0, lo1);
        let (mhf, hf) = qmf_split(hi0, hi1);
        [lf, mlf, mhf, hf]
    }
}

/// Full QMF synthesis tree: 4 subbands → 4 PCM samples.
#[derive(Clone, Debug, Default)]
pub struct QmfSynthesis {
    _s: (),
}

impl QmfSynthesis {
    pub fn new() -> Self {
        Self { _s: () }
    }

    /// Combine [LF, MLF, MHF, HF] back into 4 PCM samples.
    pub fn process(&mut self, subbands: [i32; 4]) -> [i32; 4] {
        let [lf, mlf, mhf, hf] = subbands;
        // Inner-stage join.
        let (lo0, lo1) = qmf_join(lf, mlf);
        let (hi0, hi1) = qmf_join(mhf, hf);
        // Outer-stage join.
        let (s0, s1) = qmf_join(lo0, hi0);
        let (s2, s3) = qmf_join(lo1, hi1);
        [s0, s1, s2, s3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_silence_stays_silent() {
        let mut a = QmfAnalysis::new();
        for _ in 0..16 {
            let sb = a.process([0; 4]);
            assert_eq!(sb, [0; 4]);
        }
    }

    #[test]
    fn synthesis_silence_stays_silent() {
        let mut s = QmfSynthesis::new();
        for _ in 0..16 {
            let pcm = s.process([0; 4]);
            assert_eq!(pcm, [0; 4]);
        }
    }

    #[test]
    fn analysis_synthesis_roundtrip_lossless_on_evens() {
        // Haar analysis-then-synthesis is exactly invertible when input
        // values are even (the >>1 in analysis loses the LSB on odd).
        let mut ana = QmfAnalysis::new();
        let mut syn = QmfSynthesis::new();
        for v in (-1000..1000).step_by(2) {
            let block = [v, v + 2, v + 4, v + 6];
            let sb = ana.process(block);
            let pcm = syn.process(sb);
            assert_eq!(
                pcm, block,
                "Haar QMF roundtrip failed on even-valued input {block:?} → {pcm:?}"
            );
        }
    }

    #[test]
    fn analysis_synthesis_roundtrip_low_distortion() {
        // Pure tone, full pipeline. Haar QMF is perfect-reconstruction
        // (modulo a 1-LSB rounding error per output sample), so PSNR
        // should be very high.
        let mut ana = QmfAnalysis::new();
        let mut syn = QmfSynthesis::new();
        let n_blocks = 200;
        let mut input = Vec::with_capacity(n_blocks * 4);
        for i in 0..n_blocks * 4 {
            let v =
                ((i as f64 * 2.0 * std::f64::consts::PI * 500.0 / 44100.0).sin() * 16_000.0) as i32;
            input.push(v);
        }
        let mut output = Vec::with_capacity(input.len());
        for chunk in input.chunks_exact(4) {
            let block = [chunk[0], chunk[1], chunk[2], chunk[3]];
            let sb = ana.process(block);
            let pcm = syn.process(sb);
            output.extend_from_slice(&pcm);
        }
        let mut err = 0.0f64;
        let mut sig = 0.0f64;
        for i in 0..input.len() {
            let x = input[i] as f64;
            let y = output[i] as f64;
            err += (x - y).powi(2);
            sig += x * x;
        }
        let psnr = if err > 0.0 {
            10.0 * (sig / err).log10()
        } else {
            200.0
        };
        assert!(
            psnr > 40.0,
            "Haar QMF roundtrip too lossy on LF tone: PSNR {psnr:.2} dB"
        );
    }
}
