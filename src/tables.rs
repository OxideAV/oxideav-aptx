//! Per-subband per-variant ADPCM tables for aptX.
//!
//! ## Clean-room placeholder warning
//!
//! Per the trace doc (§9.1), the **numerical contents** of the
//! quantizer-interval, dither, and step-size tables are
//! Qualcomm-specified under NDA — the trace doc deliberately omits
//! them, and this crate cannot legally transcribe them from the
//! upstream open-source reference (libavcodec / openaptx) under the
//! workspace's clean-room policy.
//!
//! What this module ships are **structurally valid placeholder
//! tables** of the correct sizes:
//!
//! | Subband | Classic table size | HD table size |
//! |---------|------------------:|--------------:|
//! | LF      |  65 (= 2^6+1)     |  257 (= 2^8+1)|
//! | MLF     |   9 (= 2^3+1)     |   33 (= 2^5+1)|
//! | MHF     |   3 (= 2^1+1)     |    9 (= 2^3+1)|
//! | HF      |   5 (= 2^2+1)     |   17 (= 2^4+1)|
//!
//! The placeholder values follow a smooth power-law spacing chosen so
//! that the encoder ↔ decoder pair in this crate self-roundtrips
//! cleanly. They will produce a different on-the-wire byte stream
//! from FFmpeg's `aptx`/`aptx_hd` — bit-exact interop is gated on
//! obtaining the real Qualcomm tables. Replacing only the table
//! constants in this file is sufficient to flip the codec to
//! interop mode.

/// Variant tag — selects between aptX classic (16-bit/codeword) and
/// aptX HD (24-bit/codeword).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Variant {
    Classic,
    Hd,
}

impl Variant {
    /// Block size on the wire (one stereo block).
    pub const fn block_bytes(self) -> usize {
        match self {
            Variant::Classic => 4,
            Variant::Hd => 6,
        }
    }
    /// Bits per channel per block.
    pub const fn bits_per_channel(self) -> usize {
        match self {
            Variant::Classic => 16,
            Variant::Hd => 24,
        }
    }
}

/// Subband index — must be 0/1/2/3.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Subband {
    Lf = 0,
    Mlf = 1,
    Mhf = 2,
    Hf = 3,
}

impl Subband {
    pub const fn from_index(i: usize) -> Self {
        match i {
            0 => Subband::Lf,
            1 => Subband::Mlf,
            2 => Subband::Mhf,
            _ => Subband::Hf,
        }
    }
    /// Bits used for this subband's codeword.
    pub const fn bits(self, variant: Variant) -> usize {
        match (variant, self) {
            (Variant::Classic, Subband::Lf) => 7,
            (Variant::Classic, Subband::Mlf) => 4,
            (Variant::Classic, Subband::Mhf) => 2,
            (Variant::Classic, Subband::Hf) => 3,
            (Variant::Hd, Subband::Lf) => 9,
            (Variant::Hd, Subband::Mlf) => 6,
            (Variant::Hd, Subband::Mhf) => 4,
            (Variant::Hd, Subband::Hf) => 5,
        }
    }
    /// Prediction order N per the trace doc (§4.3 table).
    pub const fn prediction_order(self) -> usize {
        match self {
            Subband::Lf => 24,
            Subband::Mlf => 12,
            Subband::Mhf => 6,
            Subband::Hf => 12,
        }
    }
    /// Maximum factor_select (cap, per-subband). Clean-room placeholder
    /// caps; the real Qualcomm caps may differ but the structural
    /// behaviour is identical. Set to span the full 32-entry
    /// QUANTIZATION_FACTORS table so the encoder can adapt up to a
    /// 24-bit-effective signal range.
    pub const fn factor_max(self) -> i32 {
        match self {
            Subband::Lf => 31,
            Subband::Mlf => 31,
            Subband::Mhf => 31,
            Subband::Hf => 31,
        }
    }
}

/// 32-entry common quantization-factor table. Geometric spacing — each
/// entry is roughly 2× the previous every 4 indices, covering 5 orders
/// of magnitude. This is a placeholder ladder; the Qualcomm-specified
/// table has the same overall shape but exact numbers are NDA. Encoder
/// and decoder use the same indices so as long as both agree,
/// self-roundtrip works.
pub const QUANTIZATION_FACTORS: [i32; 32] = {
    let mut t = [0i32; 32];
    let mut i = 0;
    while i < 32 {
        // Geometric ladder, base 2^(1/4): t[i] ≈ 100 × 2^(i/4).
        // Start at 100 so even factor_select=0 gives a non-trivial step
        // size — this is what lets the placeholder Jayant adaptation
        // converge in the first ~10 blocks instead of taking 1000s.
        let bump = match i {
            0 => 100,
            1 => 119,
            2 => 141,
            3 => 168,
            4 => 200,
            5 => 238,
            6 => 283,
            7 => 336,
            8 => 400,
            9 => 476,
            10 => 566,
            11 => 673,
            12 => 800,
            13 => 951,
            14 => 1131,
            15 => 1345,
            16 => 1600,
            17 => 1903,
            18 => 2263,
            19 => 2691,
            20 => 3200,
            21 => 3805,
            22 => 4525,
            23 => 5382,
            24 => 6400,
            25 => 7611,
            26 => 9051,
            27 => 10765,
            28 => 12800,
            29 => 15222,
            30 => 18102,
            31 => 21530,
            _ => 1,
        };
        t[i] = bump;
        i += 1;
    }
    t
};

/// Build a smoothly-spaced 1 + 2^(bits-1)-entry quantizer-interval table.
/// `bits` is the **signed** codeword width, so the table is indexed by
/// the codeword's magnitude (0..=2^(bits-1)). The trace doc (§9.1)
/// specifies these sizes per subband: classic LF=65, MLF=9, MHF=3,
/// HF=5; HD LF=257, MLF=33, MHF=9, HF=17.
///
/// The value at index `i` is the threshold for the i-th interval (in
/// arbitrary units; the encoder scales these by `quantization_factor`
/// to get real comparison thresholds). Quadratic spacing: smooth and
/// monotonic so a search over them is unambiguous.
pub fn make_interval_table(bits: usize) -> Vec<i32> {
    let n = (1usize << (bits - 1)) + 1;
    let mut t = Vec::with_capacity(n);
    for i in 0..n {
        let v = (i as i64) * (i as i64);
        t.push(v as i32);
    }
    t
}

/// Build a per-interval dither factor table (signed, used to smear
/// residual quantizer error at decode time). Placeholder: alternating
/// ±k where k grows with interval index. Same size as the matching
/// interval table (1 + 2^(bits-1)).
pub fn make_dither_factors(bits: usize) -> Vec<i32> {
    let n = (1usize << (bits - 1)) + 1;
    (0..n)
        .map(|i| {
            let mag = (i as i32) * 3;
            if i & 1 == 0 {
                mag
            } else {
                -mag
            }
        })
        .collect()
}

/// Build the per-interval factor-select offset table — the small
/// signed offsets the codec adds to `factor_select` after each
/// quantization step (this is the "Jayant adaptive step-size update"
/// rule). Placeholder: linear ramp from -2 (smallest interval) up to
/// +N (largest interval), encouraging step-size growth on big
/// residuals and shrink on small ones. Same size as the interval
/// table.
pub fn make_factor_select_offsets(bits: usize) -> Vec<i32> {
    let n = (1usize << (bits - 1)) + 1;
    let quarter = (n / 4) as i32;
    (0..n).map(|i| (i as i32) - quarter).collect()
}

/// Bundle of per-subband tables for a given (variant, subband) pair.
#[derive(Clone, Debug)]
pub struct SubbandTables {
    pub intervals: Vec<i32>,
    pub dither_factors: Vec<i32>,
    pub factor_select_offsets: Vec<i32>,
}

impl SubbandTables {
    pub fn new(variant: Variant, sb: Subband) -> Self {
        let bits = sb.bits(variant);
        Self {
            intervals: make_interval_table(bits),
            dither_factors: make_dither_factors(bits),
            factor_select_offsets: make_factor_select_offsets(bits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_table_sizes_match_trace_doc() {
        // Per trace doc §9.1: LF=65, MLF=9, MHF=3, HF=5.
        let expected: [(Subband, usize); 4] = [
            (Subband::Lf, 65),
            (Subband::Mlf, 9),
            (Subband::Mhf, 3),
            (Subband::Hf, 5),
        ];
        for (sb, n) in expected {
            let t = SubbandTables::new(Variant::Classic, sb);
            assert_eq!(t.intervals.len(), n, "{sb:?} intervals");
            assert_eq!(t.dither_factors.len(), n, "{sb:?} dither");
            assert_eq!(t.factor_select_offsets.len(), n, "{sb:?} fs_offsets");
        }
    }

    #[test]
    fn hd_table_sizes_match_trace_doc() {
        // Per trace doc §9.1: LF=257, MLF=33, MHF=9, HF=17.
        let expected: [(Subband, usize); 4] = [
            (Subband::Lf, 257),
            (Subband::Mlf, 33),
            (Subband::Mhf, 9),
            (Subband::Hf, 17),
        ];
        for (sb, n) in expected {
            let t = SubbandTables::new(Variant::Hd, sb);
            assert_eq!(t.intervals.len(), n, "{sb:?} intervals");
        }
    }

    #[test]
    fn intervals_are_monotonic() {
        for v in [Variant::Classic, Variant::Hd] {
            for sb in [Subband::Lf, Subband::Mlf, Subband::Mhf, Subband::Hf] {
                let t = SubbandTables::new(v, sb);
                for w in t.intervals.windows(2) {
                    assert!(w[0] <= w[1], "intervals not monotonic in {v:?}/{sb:?}");
                }
            }
        }
    }

    #[test]
    fn variant_block_sizes() {
        assert_eq!(Variant::Classic.block_bytes(), 4);
        assert_eq!(Variant::Hd.block_bytes(), 6);
        assert_eq!(Variant::Classic.bits_per_channel(), 16);
        assert_eq!(Variant::Hd.bits_per_channel(), 24);
    }

    #[test]
    fn quantization_factors_are_monotonic_increasing() {
        for w in QUANTIZATION_FACTORS.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }
}
