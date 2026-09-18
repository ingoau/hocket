//! Encoder delay / padding headers for gapless playback.
//!
//! Lossy encoders add silence: MP3 has a decoder-side delay of 529 samples
//! plus whatever the encoder's filter bank added (typically 576), and pads the
//! last frame; AAC primes its filter bank with 2112 samples (or 1024 + 2112
//! with SBR). Without trimming, every album side has an audible gap and the
//! track boundary is off. Two headers record the amounts:
//!
//! - **LAME / Xing** in the first MP3 frame: the `Xing`/`Info` tag followed by
//!   the LAME extension with 12-bit encoder delay and padding fields.
//! - **iTunSMPB** freeform atom in MP4/M4A (`com.apple.iTunes:iTunSMPB`): a
//!   string of hex fields with priming, padding and valid sample count.
//!
//! FLAC and other lossless formats carry exact sample counts and have nothing
//! to trim. Both parsers here are pure so they can be unit-tested with
//! synthetic bytes; the native decoder applies the result as frame trims.

/// Samples to drop at the start and end of the decoded stream, in the
/// codec's own sample rate. Values are what the file declares — decoder-side
/// delay is added by [`EncoderPadding::trim_for_mp3`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncoderPadding {
    /// Samples of encoder delay (priming) at the start.
    pub delay: u32,
    /// Samples of padding at the end.
    pub padding: u32,
    /// Total valid samples if the header states it (iTunSMPB does, LAME
    /// only implies it through frame count).
    pub valid_samples: Option<u64>,
}

/// The MP3 decoder's own filter-bank delay, added to the encoder delay and
/// subtracted from the padding (the padding already accounts for it).
pub const MP3_DECODER_DELAY: u32 = 529;

/// What to skip at each end in decoded samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Trim {
    pub skip_start: u32,
    pub skip_end: u32,
}

impl EncoderPadding {
    /// Convert LAME-declared values into decoded-sample trims: the decoder's
    /// 529-sample delay is added at the start and removed from the padding.
    pub fn trim_for_mp3(self) -> Trim {
        Trim {
            skip_start: self.delay.saturating_add(MP3_DECODER_DELAY),
            skip_end: self.padding.saturating_sub(MP3_DECODER_DELAY),
        }
    }

    /// iTunSMPB values already include the codec's priming, so they map
    /// straight to trims.
    pub fn trim_for_aac(self) -> Trim {
        Trim {
            skip_start: self.delay,
            skip_end: self.padding,
        }
    }
}

/// ReplayGain values the LAME tag can carry (radio = track gain).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LameGain {
    pub track_gain_db: Option<f64>,
    pub album_gain_db: Option<f64>,
    pub peak: Option<f64>,
}

/// Everything useful out of the first MP3 frame's Xing/Info + LAME tag.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LameInfo {
    pub padding: EncoderPadding,
    /// Total frame count from the Xing tag, when present.
    pub frames: Option<u32>,
    pub gain: LameGain,
    /// `true` for a CBR "Info" tag, `false` for "Xing" (VBR).
    pub is_cbr: bool,
}

/// Parsed MPEG audio frame header fields needed to locate the Xing tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameHeader {
    mpeg1: bool,
    mono: bool,
    crc: bool,
}

fn parse_frame_header(b: &[u8]) -> Option<FrameHeader> {
    if b.len() < 4 || b[0] != 0xFF || (b[1] & 0xE0) != 0xE0 {
        return None;
    }
    let version = (b[1] >> 3) & 0x03; // 00=2.5 01=reserved 10=2 11=1
    let layer = (b[1] >> 1) & 0x03; // 01 = layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let crc = (b[1] & 0x01) == 0;
    let channel_mode = (b[3] >> 6) & 0x03;
    Some(FrameHeader {
        mpeg1: version == 3,
        mono: channel_mode == 3,
        crc,
    })
}

/// Offset of the Xing/Info tag from the start of the frame.
fn xing_offset(h: FrameHeader) -> usize {
    let side_info = match (h.mpeg1, h.mono) {
        (true, false) => 32,
        (true, true) => 17,
        (false, false) => 17,
        (false, true) => 9,
    };
    4 + if h.crc { 2 } else { 0 } + side_info
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Parse the first frame of an MP3 (bytes from the frame sync, after any
/// ID3v2 tag). Returns `None` when there is no Xing/Info tag or the frame is
/// not Layer III. The LAME extension is optional: a Xing tag without it
/// yields zero padding but a frame count.
pub fn parse_lame_header(frame: &[u8]) -> Option<LameInfo> {
    let header = parse_frame_header(frame)?;
    let off = xing_offset(header);
    let tag = frame.get(off..off + 8)?;
    let is_cbr = match &tag[..4] {
        b"Xing" => false,
        b"Info" => true,
        _ => return None,
    };
    let flags = be_u32(&tag[4..8]);
    let mut pos = off + 8;
    let mut frames = None;
    if flags & 0x1 != 0 {
        frames = Some(be_u32(frame.get(pos..pos + 4)?));
        pos += 4;
    }
    if flags & 0x2 != 0 {
        pos += 4; // byte count
    }
    if flags & 0x4 != 0 {
        pos += 100; // TOC
    }
    if flags & 0x8 != 0 {
        pos += 4; // quality
    }
    let mut info = LameInfo {
        frames,
        is_cbr,
        ..Default::default()
    };
    // LAME extension: 9-byte version string starting "LAME" (or "Lavc"/"Lavf"
    // for ffmpeg, which writes the same layout).
    let Some(ext) = frame.get(pos..pos + 36) else {
        return Some(info);
    };
    if !(ext.starts_with(b"LAME")
        || ext.starts_with(b"Lavc")
        || ext.starts_with(b"Lavf")
        || ext.starts_with(b"L3.9"))
    {
        return Some(info);
    }
    // Layout relative to the version string start:
    //  0..9   version, 9 revision/VBR method, 10 lowpass,
    //  11..15 peak (9.23 fixed point, LAME >= 3.94; 0 = unset), 15..17 radio gain,
    //  17..19 audiophile gain, 19 flags, 20 bitrate, 21..24 delay/padding.
    // Peak amplitude in 9.23 fixed point (1.0 = full scale).
    let peak_raw = be_u32(&ext[11..15]);
    if peak_raw != 0 {
        let peak = f64::from(peak_raw) / f64::from(1u32 << 23);
        if peak.is_finite() && peak > 0.0 && peak < 100.0 {
            info.gain.peak = Some(peak);
        }
    }
    info.gain.track_gain_db = parse_lame_gain_field(u16::from_be_bytes([ext[15], ext[16]]), 1);
    info.gain.album_gain_db = parse_lame_gain_field(u16::from_be_bytes([ext[17], ext[18]]), 2);
    let d0 = u32::from(ext[21]);
    let d1 = u32::from(ext[22]);
    let d2 = u32::from(ext[23]);
    info.padding = EncoderPadding {
        delay: (d0 << 4) | (d1 >> 4),
        padding: ((d1 & 0x0F) << 8) | d2,
        valid_samples: None,
    };
    Some(info)
}

/// A LAME ReplayGain field: 3 bits name code (1 radio, 2 audiophile), 3 bits
/// originator, 1 bit sign, 9 bits magnitude in 0.1 dB.
fn parse_lame_gain_field(v: u16, expected_name: u16) -> Option<f64> {
    if v == 0 {
        return None;
    }
    let name = v >> 13;
    if name != expected_name {
        return None;
    }
    let originator = (v >> 10) & 0x7;
    if originator == 0 {
        return None;
    }
    let negative = (v >> 9) & 0x1 == 1;
    let magnitude = f64::from(v & 0x1FF) / 10.0;
    Some(if negative { -magnitude } else { magnitude })
}

/// Parse the iTunSMPB string, e.g.
/// `" 00000000 00000840 000001C4 00000000000B3F9C 00000000 00000000 ..."`.
/// Fields: reserved, priming (delay), padding, valid samples, then reserved.
pub fn parse_itunsmpb(value: &str) -> Option<EncoderPadding> {
    let mut fields = value.split_whitespace();
    let _reserved = fields.next()?;
    let delay = u32::from_str_radix(fields.next()?, 16).ok()?;
    let padding = u32::from_str_radix(fields.next()?, 16).ok()?;
    let valid = fields.next().and_then(|f| u64::from_str_radix(f, 16).ok());
    Some(EncoderPadding {
        delay,
        padding,
        valid_samples: valid,
    })
}

/// Skip an ID3v2 tag at the start of an MP3 buffer, returning the offset of
/// the first byte after it (0 when there is none). Handles the footer flag.
pub fn skip_id3v2(bytes: &[u8]) -> usize {
    if bytes.len() < 10 || &bytes[..3] != b"ID3" {
        return 0;
    }
    let flags = bytes[5];
    let size = bytes[6..10]
        .iter()
        .fold(0usize, |acc, b| (acc << 7) | usize::from(b & 0x7F));
    let footer = if flags & 0x10 != 0 { 10 } else { 0 };
    (10 + size + footer).min(bytes.len())
}

/// Find the first MPEG frame sync at or after `start` and parse its LAME
/// header. Convenience over [`parse_lame_header`] for raw file heads.
pub fn find_lame_header(bytes: &[u8]) -> Option<LameInfo> {
    let start = skip_id3v2(bytes);
    let mut i = start;
    // Only look through the first few KiB: the tag is in the first frame.
    let limit = bytes.len().min(start + 8192);
    while i + 4 <= limit {
        if bytes[i] == 0xFF && (bytes[i + 1] & 0xE0) == 0xE0 {
            if let Some(info) = parse_lame_header(&bytes[i..]) {
                return Some(info);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a synthetic first frame: MPEG-1 Layer III stereo, optional CRC,
    /// with a Xing tag (frames + bytes + TOC + quality) and a LAME extension
    /// carrying `delay`/`padding` and a radio gain.
    pub(crate) fn synthetic_lame_frame(
        delay: u32,
        padding: u32,
        mono: bool,
        crc: bool,
        cbr: bool,
    ) -> Vec<u8> {
        let mut f = vec![0u8; 1024];
        f[0] = 0xFF;
        // version 11 (MPEG1), layer 01 (III), protection bit 0 = CRC present
        f[1] = 0xFA | if crc { 0x00 } else { 0x01 };
        f[2] = 0x90; // 128 kbps, 44.1 kHz
        f[3] = if mono { 0xC0 } else { 0x00 };
        let h = parse_frame_header(&f).unwrap();
        let off = xing_offset(h);
        f[off..off + 4].copy_from_slice(if cbr { b"Info" } else { b"Xing" });
        f[off + 4..off + 8].copy_from_slice(&0x0000_000Fu32.to_be_bytes());
        let mut p = off + 8;
        f[p..p + 4].copy_from_slice(&1234u32.to_be_bytes()); // frames
        p += 4;
        f[p..p + 4].copy_from_slice(&999_999u32.to_be_bytes()); // bytes
        p += 4;
        p += 100; // TOC
        f[p..p + 4].copy_from_slice(&50u32.to_be_bytes()); // quality
        p += 4;
        f[p..p + 9].copy_from_slice(b"LAME3.99r");
        f[p + 9] = 0x03; // revision 0, VBR method 3
        f[p + 10] = 0xC0; // lowpass
        f[p + 11..p + 15].copy_from_slice(&((0.987 * f64::from(1u32 << 23)) as u32).to_be_bytes());
        // radio gain: name 1, originator 3 (user), negative, 6.5 dB -> 65
        let radio: u16 = (1 << 13) | (3 << 10) | (1 << 9) | 65;
        f[p + 15..p + 17].copy_from_slice(&radio.to_be_bytes());
        // audiophile: name 2, originator 3, positive, 1.2 dB
        let audiophile: u16 = (2 << 13) | (3 << 10) | 12;
        f[p + 17..p + 19].copy_from_slice(&audiophile.to_be_bytes());
        f[p + 21] = (delay >> 4) as u8;
        f[p + 22] = (((delay & 0x0F) << 4) | (padding >> 8)) as u8;
        f[p + 23] = (padding & 0xFF) as u8;
        f
    }

    #[test]
    fn parses_delay_and_padding_from_synthetic_lame_tag() {
        let frame = synthetic_lame_frame(576, 1728, false, false, false);
        let info = parse_lame_header(&frame).expect("lame header");
        assert_eq!(
            info.padding,
            EncoderPadding {
                delay: 576,
                padding: 1728,
                valid_samples: None
            }
        );
        assert_eq!(info.frames, Some(1234));
        assert!(!info.is_cbr);
        assert_eq!(info.gain.track_gain_db, Some(-6.5));
        assert_eq!(info.gain.album_gain_db, Some(1.2));
        assert!((info.gain.peak.unwrap() - 0.987).abs() < 1e-6);
        assert_eq!(
            info.padding.trim_for_mp3(),
            Trim {
                skip_start: 576 + 529,
                skip_end: 1728 - 529
            }
        );
    }

    #[test]
    fn handles_mono_crc_and_cbr_layouts() {
        for (mono, crc) in [(true, false), (false, true), (true, true)] {
            let frame = synthetic_lame_frame(1105, 7, mono, crc, true);
            let info = parse_lame_header(&frame).unwrap_or_else(|| panic!("mono={mono} crc={crc}"));
            assert_eq!(info.padding.delay, 1105);
            assert_eq!(info.padding.padding, 7);
            assert!(info.is_cbr);
        }
    }

    #[test]
    fn twelve_bit_fields_saturate_at_4095() {
        let frame = synthetic_lame_frame(4095, 4095, false, false, false);
        let info = parse_lame_header(&frame).unwrap();
        assert_eq!(info.padding.delay, 4095);
        assert_eq!(info.padding.padding, 4095);
        let small = EncoderPadding {
            delay: 0,
            padding: 100,
            valid_samples: None,
        };
        assert_eq!(
            small.trim_for_mp3().skip_end,
            0,
            "padding below decoder delay saturates"
        );
    }

    #[test]
    fn xing_without_lame_extension_gives_frames_only() {
        let mut frame = synthetic_lame_frame(576, 576, false, false, false);
        let off = xing_offset(parse_frame_header(&frame).unwrap());
        let lame_at = off + 8 + 4 + 4 + 100 + 4;
        for b in &mut frame[lame_at..lame_at + 9] {
            *b = 0;
        }
        let info = parse_lame_header(&frame).unwrap();
        assert_eq!(info.frames, Some(1234));
        assert_eq!(info.padding, EncoderPadding::default());
    }

    #[test]
    fn rejects_non_layer3_and_missing_tags() {
        assert!(parse_lame_header(&[0u8; 64]).is_none());
        let mut frame = synthetic_lame_frame(1, 1, false, false, false);
        frame[1] = 0xFC; // layer bits 10 = layer II
        assert!(parse_lame_header(&frame).is_none());
        let mut frame = synthetic_lame_frame(1, 1, false, false, false);
        let off = xing_offset(parse_frame_header(&frame).unwrap());
        frame[off..off + 4].copy_from_slice(b"Nope");
        assert!(parse_lame_header(&frame).is_none());
        assert!(parse_lame_header(&frame[..8]).is_none());
    }

    #[test]
    fn find_header_skips_id3v2_and_junk() {
        let frame = synthetic_lame_frame(576, 1000, false, false, false);
        let mut file = Vec::new();
        file.extend_from_slice(b"ID3\x04\x00\x10"); // v2.4 with footer flag
        let size = 300u32;
        file.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        file.extend(std::iter::repeat_n(0xAAu8, 300 + 10)); // body + footer
        file.extend_from_slice(&[0x00, 0x00, 0x00]); // junk before sync
        file.extend_from_slice(&frame);
        let info = find_lame_header(&file).expect("found");
        assert_eq!(info.padding.delay, 576);
        assert_eq!(skip_id3v2(&file), 10 + 300 + 10);
        assert_eq!(skip_id3v2(b"no tag here"), 0);
    }

    #[test]
    fn parses_itunsmpb() {
        let s = " 00000000 00000840 000001C4 00000000000B3F9C 00000000 00000000 00000000 00000000 00000000 00000000 00000000 00000000";
        let p = parse_itunsmpb(s).unwrap();
        assert_eq!(
            p,
            EncoderPadding {
                delay: 2112,
                padding: 452,
                valid_samples: Some(0xB3F9C)
            }
        );
        assert_eq!(
            p.trim_for_aac(),
            Trim {
                skip_start: 2112,
                skip_end: 452
            }
        );
        assert!(parse_itunsmpb("garbage").is_none());
        assert!(parse_itunsmpb(" 00000000 zzzz 0001").is_none());
        // Short form without valid-sample count still parses.
        assert_eq!(
            parse_itunsmpb("00000000 00000840 00000100")
                .unwrap()
                .valid_samples,
            None
        );
    }

    #[test]
    fn lame_gain_field_edge_cases() {
        assert_eq!(parse_lame_gain_field(0, 1), None);
        assert_eq!(
            parse_lame_gain_field((2 << 13) | (1 << 10) | 5, 1),
            None,
            "wrong name code"
        );
        assert_eq!(
            parse_lame_gain_field((1 << 13) | 5, 1),
            None,
            "no originator = unset"
        );
        assert_eq!(
            parse_lame_gain_field((1 << 13) | (1 << 10) | 5, 1),
            Some(0.5)
        );
    }
}
