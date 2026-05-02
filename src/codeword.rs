//! Per-channel codeword pack/unpack for aptX (trace doc §5.2).
//!
//! Classic 16-bit layout (LSB at bit 0):
//!
//! ```text
//! bit  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
//!      |    HF     | MHF |     MLF      |       LF       |
//!         3 bits    2 b      4 bits           7 bits
//! ```
//!
//! HD 24-bit layout (LSB at bit 0):
//!
//! ```text
//! bit  23 22 21 20 19 18 17 16 15 14 13 12 11 10  9  8  7 .. 0
//!      |      HF      |    MHF    |     MLF      |     LF       |
//!         5 bits        4 bits      6 bits         9 bits
//! ```
//!
//! The two channels are packed back-to-back, big-endian, into one block.

use crate::tables::{Subband, Variant};

/// Mask the low `bits` of `v` and sign-extend if MSB is set.
fn sign_extend(v: u32, bits: usize) -> i32 {
    let mask = (1u32 << bits) - 1;
    let masked = v & mask;
    let sign_bit = 1u32 << (bits - 1);
    if masked & sign_bit != 0 {
        (masked | !mask) as i32
    } else {
        masked as i32
    }
}

/// Pack four signed codewords (one per subband, LF/MLF/MHF/HF) into a
/// single channel codeword (16 bits classic, 24 bits HD).
pub fn pack_channel(variant: Variant, codewords: [i32; 4]) -> u32 {
    let bits = [
        Subband::Lf.bits(variant),
        Subband::Mlf.bits(variant),
        Subband::Mhf.bits(variant),
        Subband::Hf.bits(variant),
    ];
    let masks = bits.map(|b| (1u32 << b) - 1);
    let mut acc: u32 = 0;
    let mut shift = 0usize;
    for i in 0..4 {
        let v = (codewords[i] as u32) & masks[i];
        acc |= v << shift;
        shift += bits[i];
    }
    acc
}

/// Unpack a single channel codeword into four signed per-subband
/// values [LF, MLF, MHF, HF].
pub fn unpack_channel(variant: Variant, raw: u32) -> [i32; 4] {
    let bits = [
        Subband::Lf.bits(variant),
        Subband::Mlf.bits(variant),
        Subband::Mhf.bits(variant),
        Subband::Hf.bits(variant),
    ];
    let mut out = [0i32; 4];
    let mut shift = 0usize;
    for i in 0..4 {
        let v = raw >> shift;
        out[i] = sign_extend(v, bits[i]);
        shift += bits[i];
    }
    out
}

/// Pack two channel codewords into a stereo block (4 B classic / 6 B HD,
/// big-endian).
pub fn pack_block(variant: Variant, left: u32, right: u32) -> Vec<u8> {
    match variant {
        Variant::Classic => {
            // 16 bits each, big-endian.
            let mut out = Vec::with_capacity(4);
            out.extend_from_slice(&((left as u16).to_be_bytes()));
            out.extend_from_slice(&((right as u16).to_be_bytes()));
            out
        }
        Variant::Hd => {
            // 24 bits each, big-endian (3 bytes per channel).
            vec![
                ((left >> 16) & 0xFF) as u8,
                ((left >> 8) & 0xFF) as u8,
                (left & 0xFF) as u8,
                ((right >> 16) & 0xFF) as u8,
                ((right >> 8) & 0xFF) as u8,
                (right & 0xFF) as u8,
            ]
        }
    }
}

/// Inverse of `pack_block` — read one stereo block from `bytes` and
/// return the (left, right) channel codewords.
pub fn unpack_block(variant: Variant, bytes: &[u8]) -> Option<(u32, u32)> {
    match variant {
        Variant::Classic => {
            if bytes.len() < 4 {
                return None;
            }
            let l = u16::from_be_bytes([bytes[0], bytes[1]]) as u32;
            let r = u16::from_be_bytes([bytes[2], bytes[3]]) as u32;
            Some((l, r))
        }
        Variant::Hd => {
            if bytes.len() < 6 {
                return None;
            }
            let l = ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32;
            let r = ((bytes[3] as u32) << 16) | ((bytes[4] as u32) << 8) | bytes[5] as u32;
            Some((l, r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_pack_unpack_roundtrip() {
        // LF=±63 max, MLF=±7, MHF=±1, HF=±3 — full-range signed values
        // for each band.
        let cw = [-63, 7, -1, 3];
        let packed = pack_channel(Variant::Classic, cw);
        let back = unpack_channel(Variant::Classic, packed);
        assert_eq!(back, cw);
    }

    #[test]
    fn hd_pack_unpack_roundtrip() {
        let cw = [-255, 31, -7, 15];
        let packed = pack_channel(Variant::Hd, cw);
        let back = unpack_channel(Variant::Hd, packed);
        assert_eq!(back, cw);
    }

    #[test]
    fn classic_block_layout_is_big_endian() {
        let bytes = pack_block(Variant::Classic, 0x1234, 0x5678);
        assert_eq!(bytes, vec![0x12, 0x34, 0x56, 0x78]);
        let (l, r) = unpack_block(Variant::Classic, &bytes).unwrap();
        assert_eq!(l, 0x1234);
        assert_eq!(r, 0x5678);
    }

    #[test]
    fn hd_block_layout_is_big_endian() {
        let bytes = pack_block(Variant::Hd, 0x123456, 0x789ABC);
        assert_eq!(bytes, vec![0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        let (l, r) = unpack_block(Variant::Hd, &bytes).unwrap();
        assert_eq!(l, 0x123456);
        assert_eq!(r, 0x789ABC);
    }

    #[test]
    fn hf_at_msb_end() {
        // Verify HF lands in the top bits — set HF=max, others=0.
        let cw = [0, 0, 0, 3]; // HF max signed = 3 in classic
        let packed = pack_channel(Variant::Classic, cw);
        // HF is 3 bits at offset 7+4+2 = 13.
        assert_eq!(packed >> 13, 3);
    }
}
