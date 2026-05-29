//! Pure-Rust aptX (classic + HD) audio codec.
//!
//! **Round 0 — clean-room rebuild scaffold (NDA-blocked).** This is a
//! fresh orphan `master`; the previous implementation was retired
//! alongside the OxideAV docs audit dated 2026-05-06. See
//! [`README.md`](https://github.com/OxideAV/oxideav-aptx/blob/master/README.md)
//! for the rebuild scope.
//!
//! ## Why this crate is a stub
//!
//! aptX (classic, HD, and Adaptive) is a Qualcomm-licensed
//! Bluetooth audio codec; the bitstream-level spec is distributed
//! under NDA and the project has not stood up a clean-room
//! workspace at `docs/audio/aptx*/`. Until a non-NDA spec
//! transcription (or an observer-trace session driven purely from
//! pre-computed fixtures) becomes available, the encode / decode
//! paths cannot be safely populated. The Implementer rounds
//! deliberately do **not** consult third-party reimplementations
//! as a cross-check — those routes are explicitly forbidden by
//! the workspace clean-room policy
//! (`docs/CLEANROOM-MANUAL.md`).
//!
//! ## What this scaffold provides
//!
//! Even while NDA-blocked at the bitstream level, the crate exposes
//! the two NDA-safe identifiers a container demuxer / muxer needs
//! to recognise an aptX-tagged stream:
//!
//! - [`CODEC_ID_STR`] — the stable codec id the framework registry
//!   will route to once the bitstream lands.
//! - [`WAVE_FORMAT_TAG_APTX`] — the RIFF/WAVE `wFormatTag` IANA
//!   registry assignment from RFC 2361 §A.24 (`0x0025`). Public
//!   registry data, not a bitstream detail; the staged source is
//!   `docs/container/riff/rfc2361-wav.txt`.
//!
//! These constants let `oxideav-avi` / `oxideav-riff` /
//! `oxideav-mp4` (etc.) tag aptX streams as `aptx` against
//! `oxideav-core`'s `CodecResolver` without taking a hard dep on
//! a populated decoder — the resolver maps tag → codec id, the
//! codec returns `Error::NotImplemented` until the bitstream is
//! unblocked, but at least the *container* side reports the right
//! codec instead of "unknown".

#![forbid(unsafe_code)]

/// Stable codec id the framework registry will route an aptX
/// stream to once the bitstream paths are populated.
///
/// Containers tagging an aptX stream against
/// `oxideav_core::CodecResolver` should use this string verbatim
/// rather than spelling `"aptx"` inline, so a future rename (e.g.
/// to distinguish `aptx-classic` / `aptx-hd`) only needs to edit
/// one place.
pub const CODEC_ID_STR: &str = "aptx";

/// RIFF/WAVE `wFormatTag` registry assignment for aptX, per IANA
/// via [RFC 2361 §A.24](https://www.rfc-editor.org/rfc/rfc2361)
/// (`docs/container/riff/rfc2361-wav.txt`).
///
/// A `WAVEFORMATEX` chunk inside an AVI `strf` / WAV `fmt ` block
/// whose `wFormatTag` field equals this value identifies the
/// payload as aptX. The constant lives here (not inside the AVI /
/// RIFF crate) because the IANA-registered identifier is intrinsic
/// to the codec — every container that ever carries aptX uses the
/// same `0x0025` tag.
pub const WAVE_FORMAT_TAG_APTX: u16 = 0x0025;

/// Crate-local error type. Concrete variants land as the
/// Implementer rounds populate the codec pipeline; until the
/// NDA-blocked spec transcription is available, the only variant
/// is [`Error::NotImplemented`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Reserved placeholder. Returned by any encode / decode entry
    /// point while the crate is in its NDA-blocked stub state.
    NotImplemented,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => f.write_str(
                "oxideav-aptx: clean-room rebuild in progress — see crates/oxideav-aptx/README.md",
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Crate-local Result alias.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_id_str_is_stable() {
        // The framework registry will key off this exact string;
        // changing it is a breaking change for every consumer that
        // pinned the tag, so we lock it down explicitly.
        assert_eq!(CODEC_ID_STR, "aptx");
    }

    #[test]
    fn wave_format_tag_matches_rfc_2361() {
        // RFC 2361 §A.24 — WAVE_FORMAT_APTX, registration number
        // 0x0025. Source: docs/container/riff/rfc2361-wav.txt.
        assert_eq!(WAVE_FORMAT_TAG_APTX, 0x0025);
    }

    #[test]
    fn error_display_points_at_readme() {
        let s = format!("{}", Error::NotImplemented);
        assert!(
            s.contains("clean-room rebuild"),
            "Error::NotImplemented Display should mention the clean-room rebuild status; got: {s}"
        );
        assert!(
            s.contains("README.md"),
            "Error::NotImplemented Display should point at the crate README; got: {s}"
        );
    }

    #[test]
    fn error_is_std_error() {
        // Compile-time check: Error implements std::error::Error.
        fn assert_error<E: std::error::Error>() {}
        assert_error::<Error>();
    }

    #[test]
    fn error_is_clone_and_eq() {
        let a = Error::NotImplemented;
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn result_alias_resolves() {
        let ok: Result<u32> = Ok(7);
        let err: Result<u32> = Err(Error::NotImplemented);
        assert_eq!(ok, Ok(7));
        assert_eq!(err, Err(Error::NotImplemented));
    }
}
