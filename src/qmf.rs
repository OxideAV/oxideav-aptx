//! Two-stage dyadic QMF (Quadrature Mirror Filter) tree for aptX.
//!
//! Per the trace doc (§4.2) and the companion sidecar
//! `docs/audio/aptx/data/aptx-qmf-coefficients.md`, aptX classic and
//! aptX HD share an identical 16-tap two-stage QMF cascade:
//!
//! - **Outer QMF** (first stage): 16-tap polyphase with two
//!   mirror-paired filter sets; splits 4 PCM samples into two
//!   intermediate sub-bands of 2 samples each.
//! - **Inner QMF** (second stage): same shape, applied to each
//!   intermediate sub-band; produces 4 final subband samples
//!   `[LF, MLF, MHF, HF]`.
//!
//! Per stage / per direction the rounded-right-shift differs:
//!
//! | Stage | Direction  | Right-shift |
//! |-------|------------|------------:|
//! | outer | analysis   | 23          |
//! | outer | synthesis  | 21          |
//! | inner | analysis   | 23          |
//! | inner | synthesis  | 22          |
//!
//! The buffer is a 32-entry ring with the 16-tap history mirrored
//! twice so the convolution reads 16 contiguous taps without modular
//! addressing.
//!
//! Coefficients are integer values from
//! `data/aptx-qmf-coefficients.md`; the same numerical values appear
//! independently in two open-source projects (FFmpeg `libavcodec/aptx.h`
//! and libopenaptx) and are reproduced as functional public data.

/// Outer QMF filter set 0 (16 taps).
pub const OUTER_COEFFS_0: [i32; 16] = [
    730, -413, -9611, 43626, -121026, 269973, -585547, 2801966, 697128, -160481, 27611, 8478,
    -10043, 3511, 688, -897,
];

/// Outer QMF filter set 1 (mirror of set 0).
pub const OUTER_COEFFS_1: [i32; 16] = [
    -897, 688, 3511, -10043, 8478, 27611, -160481, 697128, 2801966, -585547, 269973, -121026,
    43626, -9611, -413, 730,
];

/// Inner QMF filter set 0 (16 taps).
pub const INNER_COEFFS_0: [i32; 16] = [
    1033, -584, -13592, 61697, -171156, 381799, -828088, 3962579, 985888, -226954, 39048, 11990,
    -14203, 4966, 973, -1268,
];

/// Inner QMF filter set 1 (mirror of set 0).
pub const INNER_COEFFS_1: [i32; 16] = [
    -1268, 973, 4966, -14203, 11990, 39048, -226954, 985888, 3962579, -828088, 381799, -171156,
    61697, -13592, -584, 1033,
];

const SHIFT_OUTER_ANA: u32 = 23;
const SHIFT_OUTER_SYN: u32 = 21;
const SHIFT_INNER_ANA: u32 = 23;
const SHIFT_INNER_SYN: u32 = 22;

/// 24-bit signed clip, taking an `i64` accumulator down to an `i32` in
/// the int24 representable range.
fn clip24(v: i64) -> i32 {
    const MAX: i64 = (1 << 23) - 1;
    const MIN: i64 = -(1 << 23);
    v.clamp(MIN, MAX) as i32
}

/// Rounded right-shift of an `i64` accumulator by `bits`. Adds the
/// half-LSB (`1 << (bits-1)`) before the shift so the result rounds to
/// nearest rather than truncating toward `-inf`.
fn rshift_round(v: i64, bits: u32) -> i64 {
    let half = 1i64 << (bits - 1);
    (v + half) >> bits
}

/// One 16-tap polyphase QMF filter signal: 32-entry ring buffer with
/// mirrored history, plus a `pos` cursor in `[0, 16)`. Each stored
/// sample is written twice (`buf[pos]` and `buf[pos+16]`) so a 16-tap
/// convolution can always be read as `buf[pos .. pos+16]`.
#[derive(Clone, Debug)]
struct FilterSignal {
    buf: [i32; 32],
    pos: usize,
}

impl FilterSignal {
    fn new() -> Self {
        Self {
            buf: [0; 32],
            pos: 0,
        }
    }

    /// Push a new sample into the history. Writes `pos` and `pos+16`,
    /// then advances `pos` (wrapping at 16).
    #[inline(always)]
    fn push(&mut self, s: i32) {
        self.buf[self.pos] = s;
        self.buf[self.pos + 16] = s;
        self.pos = (self.pos + 1) & 15;
    }

    /// Convolve the current 16-tap window with `coeffs` into an `i64`
    /// accumulator. Read order: most recent sample × `coeffs[0]`,
    /// next-most-recent × `coeffs[1]`, etc. The mirrored buffer means
    /// the contiguous read `buf[pos + 15 - k]` (k = 0..15) walks the
    /// 16-sample history newest → oldest in increasing memory order.
    #[inline(always)]
    fn conv(&self, coeffs: &[i32; 16]) -> i64 {
        let mut acc: i64 = 0;
        let p = self.pos;
        for k in 0..16 {
            acc += coeffs[k] as i64 * self.buf[p + 15 - k] as i64;
        }
        acc
    }
}

/// One QMF analysis stage: one filter signal, two filter sets, two
/// outputs (low / high) per pair of input samples.
#[derive(Clone, Debug)]
struct AnalysisStage {
    sig: FilterSignal,
    coeffs0: &'static [i32; 16],
    coeffs1: &'static [i32; 16],
    shift: u32,
}

impl AnalysisStage {
    fn new_outer() -> Self {
        Self {
            sig: FilterSignal::new(),
            coeffs0: &OUTER_COEFFS_0,
            coeffs1: &OUTER_COEFFS_1,
            shift: SHIFT_OUTER_ANA,
        }
    }
    fn new_inner() -> Self {
        Self {
            sig: FilterSignal::new(),
            coeffs0: &INNER_COEFFS_0,
            coeffs1: &INNER_COEFFS_1,
            shift: SHIFT_INNER_ANA,
        }
    }

    /// Process one pair of input samples → (low, high).
    fn process(&mut self, s0: i32, s1: i32) -> (i32, i32) {
        // Push both samples in time order (s0 first, then s1).
        self.sig.push(s0);
        self.sig.push(s1);
        // Run two convolutions, then half-band butterfly.
        // Per the spec: "two outputs which are then summed (low) and
        // differenced (high)". Compute one convolution per filter,
        // then sum and difference.
        let a = self.sig.conv(self.coeffs0);
        let b = self.sig.conv(self.coeffs1);
        let low = clip24(rshift_round(a + b, self.shift));
        let high = clip24(rshift_round(a - b, self.shift));
        (low, high)
    }
}

/// One QMF synthesis stage: two filter signals, two filter sets, two
/// outputs per (low, high) pair. Each branch of the inverse butterfly
/// (low+high, low-high) drives its own filter signal; convolutions are
/// cross-paired (sum-branch through `coeffs1`, diff-branch through
/// `coeffs0`) so the cascade approximates perfect reconstruction.
#[derive(Clone, Debug)]
struct SynthesisStage {
    sig0: FilterSignal,
    sig1: FilterSignal,
    coeffs0: &'static [i32; 16],
    coeffs1: &'static [i32; 16],
    shift: u32,
}

impl SynthesisStage {
    fn new_outer() -> Self {
        Self {
            sig0: FilterSignal::new(),
            sig1: FilterSignal::new(),
            coeffs0: &OUTER_COEFFS_0,
            coeffs1: &OUTER_COEFFS_1,
            shift: SHIFT_OUTER_SYN,
        }
    }
    fn new_inner() -> Self {
        Self {
            sig0: FilterSignal::new(),
            sig1: FilterSignal::new(),
            coeffs0: &INNER_COEFFS_0,
            coeffs1: &INNER_COEFFS_1,
            shift: SHIFT_INNER_SYN,
        }
    }

    /// Process one (low, high) pair → 2 output samples.
    fn process(&mut self, low: i32, high: i32) -> (i32, i32) {
        // Inverse butterfly: branch0 = low+high, branch1 = low-high.
        let b0 = low + high;
        let b1 = low - high;
        // Each branch drives its own filter signal.
        self.sig0.push(b0);
        self.sig1.push(b1);
        // Cross-pair convolutions: sum-branch through filter1, diff-
        // branch through filter0. This pairing is what gives the cascade
        // its near-PR property (analysis and synthesis filters partner
        // by their mirror complement).
        let a = self.sig0.conv(self.coeffs1);
        let b = self.sig1.conv(self.coeffs0);
        let s0 = clip24(rshift_round(a, self.shift));
        let s1 = clip24(rshift_round(b, self.shift));
        (s0, s1)
    }
}

/// Full QMF analysis tree: 4 PCM samples in, 4 subband samples out
/// in `[LF, MLF, MHF, HF]` order.
#[derive(Clone, Debug)]
pub struct QmfAnalysis {
    outer: AnalysisStage,
    inner_low: AnalysisStage,
    inner_high: AnalysisStage,
}

impl Default for QmfAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl QmfAnalysis {
    pub fn new() -> Self {
        Self {
            outer: AnalysisStage::new_outer(),
            inner_low: AnalysisStage::new_inner(),
            inner_high: AnalysisStage::new_inner(),
        }
    }

    /// Split 4 input samples into `[LF, MLF, MHF, HF]`.
    pub fn process(&mut self, samples: [i32; 4]) -> [i32; 4] {
        // Outer stage: 2 step calls, each consuming a pair of PCM samples
        // and emitting (low_outer, high_outer).
        let (lo_a, hi_a) = self.outer.process(samples[0], samples[1]);
        let (lo_b, hi_b) = self.outer.process(samples[2], samples[3]);
        // Inner-low stage processes the two outer-low samples → (LF, MLF).
        let (lf, mlf) = self.inner_low.process(lo_a, lo_b);
        // Inner-high stage processes the two outer-high samples → (MHF, HF).
        let (mhf, hf) = self.inner_high.process(hi_a, hi_b);
        [lf, mlf, mhf, hf]
    }
}

/// Full QMF synthesis tree: 4 subbands → 4 PCM samples.
#[derive(Clone, Debug)]
pub struct QmfSynthesis {
    outer: SynthesisStage,
    inner_low: SynthesisStage,
    inner_high: SynthesisStage,
}

impl Default for QmfSynthesis {
    fn default() -> Self {
        Self::new()
    }
}

impl QmfSynthesis {
    pub fn new() -> Self {
        Self {
            outer: SynthesisStage::new_outer(),
            inner_low: SynthesisStage::new_inner(),
            inner_high: SynthesisStage::new_inner(),
        }
    }

    /// Combine `[LF, MLF, MHF, HF]` back into 4 PCM samples.
    pub fn process(&mut self, subbands: [i32; 4]) -> [i32; 4] {
        let [lf, mlf, mhf, hf] = subbands;
        // Inner-low joins (LF, MLF) → 2 outer-low samples.
        let (lo_a, lo_b) = self.inner_low.process(lf, mlf);
        // Inner-high joins (MHF, HF) → 2 outer-high samples.
        let (hi_a, hi_b) = self.inner_high.process(mhf, hf);
        // Outer joins each (lo_outer, hi_outer) → PCM pair.
        let (s0, s1) = self.outer.process(lo_a, hi_a);
        let (s2, s3) = self.outer.process(lo_b, hi_b);
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
    fn outer_filters_are_mirror_pair() {
        for k in 0..16 {
            assert_eq!(
                OUTER_COEFFS_1[k],
                OUTER_COEFFS_0[15 - k],
                "outer mirror property fails at k={k}"
            );
        }
    }

    #[test]
    fn inner_filters_are_mirror_pair() {
        for k in 0..16 {
            assert_eq!(
                INNER_COEFFS_1[k],
                INNER_COEFFS_0[15 - k],
                "inner mirror property fails at k={k}"
            );
        }
    }

    #[test]
    fn outer_dc_sums_agree_between_mirrored_pair() {
        // Per the spec sidecar §sanity-check: the DC sum of set 0 equals
        // that of set 1 (consequence of the mirror property). Both sets
        // are independent transcriptions of the same 16 numerical
        // coefficients, so this verifies neither was mistyped.
        // (The sidecar's stated numerical sum value of 2 798 053 is a
        // documentation typo; the actual coefficient sum is 2 965 693
        // and matches the values published byte-identically in
        // FFmpeg `aptx.h` and libopenaptx.)
        let s0: i64 = OUTER_COEFFS_0.iter().map(|&c| c as i64).sum();
        let s1: i64 = OUTER_COEFFS_1.iter().map(|&c| c as i64).sum();
        assert_eq!(s0, s1);
    }

    #[test]
    fn inner_dc_sums_agree_between_mirrored_pair() {
        // Same property as outer: spec sanity-check stated 3 962 244;
        // actual sum is 4 194 128. Both sets agree, confirming the
        // mirror-pair transcription is consistent.
        let s0: i64 = INNER_COEFFS_0.iter().map(|&c| c as i64).sum();
        let s1: i64 = INNER_COEFFS_1.iter().map(|&c| c as i64).sum();
        assert_eq!(s0, s1);
    }

    #[test]
    fn outer_centre_tap_matches_spec() {
        // Per the spec sidecar: dominant tap (index 7) of outer set 0 = 2 801 966.
        assert_eq!(OUTER_COEFFS_0[7], 2_801_966);
    }

    #[test]
    fn inner_centre_tap_matches_spec() {
        // Per the spec sidecar: dominant tap (index 7) of inner set 0 = 3 962 579.
        assert_eq!(INNER_COEFFS_0[7], 3_962_579);
    }

    #[test]
    fn analysis_synthesis_roundtrip_low_distortion() {
        // Pure tone through analysis → synthesis. With the spec 16-tap
        // coefficients the cascade approximates perfect reconstruction
        // (the two-stage 16-tap filter has a group delay of ~66 samples
        // so we search over a small delay range).
        let mut ana = QmfAnalysis::new();
        let mut syn = QmfSynthesis::new();
        let n_blocks = 500;
        let mut input = Vec::with_capacity(n_blocks * 4);
        for i in 0..n_blocks * 4 {
            let v = ((i as f64 * 2.0 * std::f64::consts::PI * 500.0 / 44100.0).sin()
                * 16_000.0
                * 256.0) as i32;
            input.push(v);
        }
        let mut output = Vec::with_capacity(input.len());
        for chunk in input.chunks_exact(4) {
            let block = [chunk[0], chunk[1], chunk[2], chunk[3]];
            let sb = ana.process(block);
            let pcm = syn.process(sb);
            output.extend_from_slice(&pcm);
        }
        let head = 200;
        let mut best_psnr = f64::NEG_INFINITY;
        for delay in 0..150 {
            if head + delay >= input.len() {
                break;
            }
            let n = input.len() - head - delay;
            let mut err = 0.0f64;
            let mut sig = 0.0f64;
            for i in 0..n {
                let x = input[head + i] as f64;
                let y = output[head + delay + i] as f64;
                err += (x - y).powi(2);
                sig += x * x;
            }
            let p = if err > 0.0 {
                10.0 * (sig / err).log10()
            } else {
                200.0
            };
            if p > best_psnr {
                best_psnr = p;
            }
        }
        assert!(
            best_psnr > 25.0,
            "QMF analysis-synthesis roundtrip too lossy: best PSNR {best_psnr:.2} dB"
        );
    }
}
