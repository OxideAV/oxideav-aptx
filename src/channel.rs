//! Per-channel state: 4 subband states + dither generator + QMF
//! analysis/synthesis trees.

use crate::dither::DitherGen;
use crate::qmf::{QmfAnalysis, QmfSynthesis};
use crate::subband::SubbandState;
use crate::tables::{Subband, Variant};

#[derive(Clone, Debug)]
pub struct EncoderChannel {
    pub variant: Variant,
    pub bands: [SubbandState; 4],
    pub qmf: QmfAnalysis,
    pub dither: DitherGen,
}

impl EncoderChannel {
    pub fn new(variant: Variant) -> Self {
        Self {
            variant,
            bands: [
                SubbandState::new(variant, Subband::Lf),
                SubbandState::new(variant, Subband::Mlf),
                SubbandState::new(variant, Subband::Mhf),
                SubbandState::new(variant, Subband::Hf),
            ],
            qmf: QmfAnalysis::new(),
            dither: DitherGen::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DecoderChannel {
    pub variant: Variant,
    pub bands: [SubbandState; 4],
    pub qmf: QmfSynthesis,
    pub dither: DitherGen,
}

impl DecoderChannel {
    pub fn new(variant: Variant) -> Self {
        Self {
            variant,
            bands: [
                SubbandState::new(variant, Subband::Lf),
                SubbandState::new(variant, Subband::Mlf),
                SubbandState::new(variant, Subband::Mhf),
                SubbandState::new(variant, Subband::Hf),
            ],
            qmf: QmfSynthesis::new(),
            dither: DitherGen::new(),
        }
    }
}
