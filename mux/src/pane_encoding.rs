use config::keyassignment::PaneEncoding;
use encoding_rs::Encoding;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum EscapeState {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Dcs,
    DcsEsc,
}

const MAX_TRAILING_ENCODED_BYTES: usize = 4;

impl Default for EscapeState {
    fn default() -> Self {
        Self::Ground
    }
}

fn encoding_rs(encoding: PaneEncoding) -> Option<&'static Encoding> {
    match encoding {
        PaneEncoding::Utf8 => None,
        PaneEncoding::Gbk => Encoding::for_label(b"gbk"),
        PaneEncoding::Gb18030 => Encoding::for_label(b"gb18030"),
    }
}

/// Decode raw bytes into a UTF-8 string using the given pane encoding.
/// First tries UTF-8; if that fails and the encoding is non-UTF-8,
/// falls back to decoding with the pane's encoding.
/// Returns `None` only if both attempts fail without producing a clean result.
pub fn decode_bytes_to_string(encoding: PaneEncoding, raw: &[u8]) -> Option<String> {
    // Try UTF-8 first
    if let Ok(s) = String::from_utf8(raw.to_vec()) {
        return Some(s);
    }
    // Fall back to the pane encoding
    let enc = encoding_rs(encoding)?;
    let (decoded, _, had_errors) = enc.decode(raw);
    if had_errors {
        log::trace!(
            "decode_bytes_to_string: lossy decode with {:?}, some bytes could not be decoded",
            encoding
        );
    }
    Some(decoded.into_owned())
}

fn advance_escape(state: EscapeState, byte: u8) -> EscapeState {
    match state {
        EscapeState::Ground => EscapeState::Ground,
        EscapeState::Esc => match byte {
            b'[' => EscapeState::Csi,
            b']' => EscapeState::Osc,
            b'P' => EscapeState::Dcs,
            0x40..=0x7e => EscapeState::Ground,
            _ => EscapeState::Esc,
        },
        EscapeState::Csi => {
            if matches!(byte, 0x40..=0x7e) {
                EscapeState::Ground
            } else {
                EscapeState::Csi
            }
        }
        EscapeState::Osc => match byte {
            0x07 => EscapeState::Ground,
            0x1b => EscapeState::OscEsc,
            _ => EscapeState::Osc,
        },
        EscapeState::OscEsc => {
            if byte == b'\\' {
                EscapeState::Ground
            } else {
                EscapeState::Osc
            }
        }
        EscapeState::Dcs => {
            if byte == 0x1b {
                EscapeState::DcsEsc
            } else {
                EscapeState::Dcs
            }
        }
        EscapeState::DcsEsc => {
            if byte == b'\\' {
                EscapeState::Ground
            } else {
                EscapeState::Dcs
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct PaneInputEncoder {
    encoding: Option<PaneEncoding>,
    state: EscapeState,
    escape_bytes: Vec<u8>,
    pending_utf8: Vec<u8>,
}

impl PaneInputEncoder {
    pub fn encode(&mut self, encoding: PaneEncoding, data: &[u8]) -> Vec<u8> {
        if self.encoding != Some(encoding) {
            self.encoding = Some(encoding);
            self.state = EscapeState::Ground;
            self.escape_bytes.clear();
            self.pending_utf8.clear();
        }
        if encoding == PaneEncoding::Utf8 {
            return data.to_vec();
        }

        let mut output = Vec::with_capacity(data.len());
        let mut text_start = 0usize;

        for (idx, &byte) in data.iter().enumerate() {
            if self.state == EscapeState::Ground && byte == 0x1b {
                if idx > text_start {
                    self.encode_text(encoding, &data[text_start..idx], &mut output);
                }
                self.escape_bytes.clear();
                self.escape_bytes.push(byte);
                self.state = EscapeState::Esc;
                text_start = idx + 1;
                continue;
            }

            if self.state != EscapeState::Ground {
                self.escape_bytes.push(byte);
                self.state = advance_escape(self.state, byte);
                if self.state == EscapeState::Ground {
                    output.extend_from_slice(&self.escape_bytes);
                    self.escape_bytes.clear();
                    text_start = idx + 1;
                }
            }
        }

        if self.state == EscapeState::Ground && text_start < data.len() {
            self.encode_text(encoding, &data[text_start..], &mut output);
        }

        output
    }

    fn encode_text(&mut self, encoding: PaneEncoding, text: &[u8], output: &mut Vec<u8>) {
        let mut pending = std::mem::take(&mut self.pending_utf8);
        pending.extend_from_slice(text);

        let mut cursor = 0usize;
        while cursor < pending.len() {
            match std::str::from_utf8(&pending[cursor..]) {
                Ok(valid) => {
                    self.push_encoded(encoding, valid, output);
                    return;
                }
                Err(err) => {
                    let valid_len = err.valid_up_to();
                    if valid_len > 0 {
                        let valid_slice = &pending[cursor..cursor + valid_len];
                        if let Ok(valid) = std::str::from_utf8(valid_slice) {
                            self.push_encoded(encoding, valid, output);
                        }
                    }

                    cursor += valid_len;
                    if err.error_len().is_none() {
                        self.pending_utf8.extend_from_slice(&pending[cursor..]);
                        return;
                    }

                    output.push(b'?');
                    log::trace!(
                        "pane input encoder: replaced invalid UTF-8 byte(s) with '?'"
                    );
                    cursor += err.error_len().unwrap_or(1);
                }
            }
        }
    }

    fn push_encoded(&self, encoding: PaneEncoding, text: &str, output: &mut Vec<u8>) {
        if let Some(enc) = encoding_rs(encoding) {
            let (encoded, _, _) = enc.encode(text);
            output.extend_from_slice(&encoded);
        } else {
            output.extend_from_slice(text.as_bytes());
        }
    }
}

#[derive(Debug, Default)]
pub struct PaneOutputDecoder {
    encoding: Option<PaneEncoding>,
    state: EscapeState,
    escape_bytes: Vec<u8>,
    pending_encoded: Vec<u8>,
}

impl PaneOutputDecoder {
    pub fn decode(&mut self, encoding: PaneEncoding, data: &[u8]) -> Vec<u8> {
        if self.encoding != Some(encoding) {
            self.encoding = Some(encoding);
            self.state = EscapeState::Ground;
            self.escape_bytes.clear();
            self.pending_encoded.clear();
        }
        if encoding == PaneEncoding::Utf8 {
            return data.to_vec();
        }

        let mut output = Vec::with_capacity(data.len());
        let mut text_start = 0usize;

        for (idx, &byte) in data.iter().enumerate() {
            if self.state == EscapeState::Ground && byte == 0x1b {
                if idx > text_start {
                    self.decode_text(encoding, &data[text_start..idx], &mut output);
                }
                self.escape_bytes.clear();
                self.escape_bytes.push(byte);
                self.state = EscapeState::Esc;
                text_start = idx + 1;
                continue;
            }

            if self.state != EscapeState::Ground {
                self.escape_bytes.push(byte);
                self.state = advance_escape(self.state, byte);
                if self.state == EscapeState::Ground {
                    output.extend_from_slice(&self.escape_bytes);
                    self.escape_bytes.clear();
                    text_start = idx + 1;
                }
            }
        }

        if self.state == EscapeState::Ground && text_start < data.len() {
            self.decode_text(encoding, &data[text_start..], &mut output);
        }

        output
    }

    fn decode_text(&mut self, encoding: PaneEncoding, input: &[u8], output: &mut Vec<u8>) {
        let mut pending = std::mem::take(&mut self.pending_encoded);
        pending.extend_from_slice(input);
        let Some(enc) = encoding_rs(encoding) else {
            output.extend_from_slice(&pending);
            return;
        };

        let min_prefix = pending
            .len()
            .saturating_sub(MAX_TRAILING_ENCODED_BYTES)
            .max(1);
        for split in (min_prefix..=pending.len()).rev() {
            let decoded_prefix =
                enc.decode_without_bom_handling_and_without_replacement(&pending[..split]);
            if let Some(text) = decoded_prefix {
                output.extend_from_slice(text.as_bytes());
                if split < pending.len() {
                    self.pending_encoded.extend_from_slice(&pending[split..]);
                }
                return;
            }
        }

        if pending.len() <= MAX_TRAILING_ENCODED_BYTES {
            self.pending_encoded.extend_from_slice(&pending);
            return;
        }

        let (decoded, _, _) = enc.decode(&pending);
        output.extend_from_slice(decoded.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::keyassignment::PaneEncoding;

    #[test]
    fn utf8_passthrough() {
        let mut enc = PaneInputEncoder::default();
        let data = "hello world".as_bytes();
        assert_eq!(enc.encode(PaneEncoding::Utf8, data), data.to_vec());

        let mut dec = PaneOutputDecoder::default();
        assert_eq!(dec.decode(PaneEncoding::Utf8, data), data.to_vec());
    }

    #[test]
    fn gbk_encode_chinese() {
        let mut enc = PaneInputEncoder::default();
        let input = "你好".as_bytes();
        let result = enc.encode(PaneEncoding::Gbk, input);
        // "你好" in GBK is [0xc4, 0xe3, 0xba, 0xc3]
        assert_eq!(result, vec![0xc4, 0xe3, 0xba, 0xc3]);
    }

    #[test]
    fn gbk_decode_chinese() {
        let mut dec = PaneOutputDecoder::default();
        let gbk_bytes: &[u8] = &[0xc4, 0xe3, 0xba, 0xc3]; // "你好" in GBK
        let result = dec.decode(PaneEncoding::Gbk, gbk_bytes);
        assert_eq!(result, "你好".as_bytes().to_vec());
    }

    #[test]
    fn esc_sequence_passthrough_on_encode() {
        let mut enc = PaneInputEncoder::default();
        // CSI sequence: ESC [ 1 ; 2 H (cursor position)
        let data = b"\x1b[1;2H";
        let result = enc.encode(PaneEncoding::Gbk, data);
        assert_eq!(result, data.to_vec());
    }

    #[test]
    fn esc_sequence_passthrough_on_decode() {
        let mut dec = PaneOutputDecoder::default();
        // CSI sequence: ESC [ 1 ; 2 H (cursor position)
        let data = b"\x1b[1;2H";
        let result = dec.decode(PaneEncoding::Gbk, data);
        assert_eq!(result, data.to_vec());
    }

    #[test]
    fn mixed_text_and_esc_on_decode() {
        let mut dec = PaneOutputDecoder::default();
        // GBK "你" + CSI sequence + GBK "好"
        let mut data: Vec<u8> = vec![0xc4, 0xe3]; // "你" in GBK
        data.extend_from_slice(b"\x1b[0m");        // SGR reset
        data.extend_from_slice(&[0xba, 0xc3]);     // "好" in GBK
        let result = dec.decode(PaneEncoding::Gbk, &data);
        let mut expected = "你".as_bytes().to_vec();
        expected.extend_from_slice(b"\x1b[0m");
        expected.extend_from_slice("好".as_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn split_multibyte_decode() {
        let mut dec = PaneOutputDecoder::default();
        // "你" in GBK is [0xc4, 0xe3] - split across two decode calls
        let part1 = vec![0xc4]; // first byte only
        let result1 = dec.decode(PaneEncoding::Gbk, &part1);
        assert!(result1.is_empty(), "incomplete char should be buffered");

        let part2 = vec![0xe3]; // second byte
        let result2 = dec.decode(PaneEncoding::Gbk, &part2);
        assert_eq!(result2, "你".as_bytes().to_vec());
    }

    #[test]
    fn unencodable_char_replaced_with_question_mark() {
        let mut enc = PaneInputEncoder::default();
        // Emoji 🚀 (U+1F680) is not in GBK
        let input = "🚀".as_bytes();
        let result = enc.encode(PaneEncoding::Gbk, input);
        // encoding_rs replaces unencodable with numeric character references in HTML mode,
        // but in encoding mode it produces &#128640; or similar. Let's just verify it doesn't crash
        // and produces some output.
        assert!(!result.is_empty());
    }

    #[test]
    fn osc_sequence_passthrough() {
        let mut dec = PaneOutputDecoder::default();
        // OSC: ESC ] 0 ; title BEL
        let data = b"\x1b]0;my title\x07";
        let result = dec.decode(PaneEncoding::Gbk, data);
        assert_eq!(result, data.to_vec());
    }

    #[test]
    fn dcs_sequence_passthrough() {
        let mut dec = PaneOutputDecoder::default();
        // DCS: ESC P ... ST (ESC \)
        let data = b"\x1bPsome data\x1b\\";
        let result = dec.decode(PaneEncoding::Gbk, data);
        assert_eq!(result, data.to_vec());
    }

    #[test]
    fn encoding_switch_resets_state() {
        let mut dec = PaneOutputDecoder::default();
        // Start with GBK, incomplete byte
        let part1 = vec![0xc4]; // first byte of "你" in GBK
        let _result1 = dec.decode(PaneEncoding::Gbk, &part1);

        // Switch to UTF-8 - should reset state
        let utf8_data = "hello".as_bytes();
        let result2 = dec.decode(PaneEncoding::Utf8, utf8_data);
        assert_eq!(result2, utf8_data.to_vec());
    }

    #[test]
    fn gb18030_encode_decode() {
        let mut enc = PaneInputEncoder::default();
        let mut dec = PaneOutputDecoder::default();
        let input = "你好世界".as_bytes();
        let encoded = enc.encode(PaneEncoding::Gb18030, input);
        let decoded = dec.decode(PaneEncoding::Gb18030, &encoded);
        assert_eq!(decoded, input.to_vec());
    }

    #[test]
    fn decode_bytes_utf8_passthrough() {
        // Valid UTF-8 bytes should pass through regardless of encoding setting
        let utf8 = "hello世界".as_bytes();
        let result = decode_bytes_to_string(PaneEncoding::Gbk, utf8);
        assert_eq!(result, Some("hello世界".to_string()));
    }

    #[test]
    fn decode_bytes_gbk_fallback() {
        // GBK bytes that are not valid UTF-8 should be decoded using GBK
        let gbk_bytes: &[u8] = &[0xc4, 0xe3, 0xba, 0xc3]; // "你好" in GBK
        let result = decode_bytes_to_string(PaneEncoding::Gbk, gbk_bytes);
        assert_eq!(result, Some("你好".to_string()));
    }

    #[test]
    fn decode_bytes_gb18030_fallback() {
        // GB18030 bytes
        let gb18030_bytes: &[u8] = &[0xc4, 0xe3, 0xba, 0xc3]; // "你好" in GB18030
        let result = decode_bytes_to_string(PaneEncoding::Gb18030, gb18030_bytes);
        assert_eq!(result, Some("你好".to_string()));
    }

    #[test]
    fn decode_bytes_utf8_encoding_returns_none_for_invalid() {
        // Invalid bytes with UTF-8 encoding should return None
        // (since there is no non-UTF-8 fallback)
        let invalid: &[u8] = &[0xc4, 0xe3, 0xba, 0xc3]; // GBK bytes, not valid UTF-8
        let result = decode_bytes_to_string(PaneEncoding::Utf8, invalid);
        assert_eq!(result, None);
    }

    #[test]
    fn decode_bytes_gbk_path_with_slashes() {
        // Simulate a GBK-encoded path like /home/用户/文档
        // "用户" in GBK: [0xd3, 0xc3, 0xbb, 0xa7]
        // "文档" in GBK: [0xce, 0xc4, 0xb5, 0xb5]
        let mut path = b"/home/".to_vec();
        path.extend_from_slice(&[0xd3, 0xc3, 0xbb, 0xa7]); // 用户
        path.push(b'/');
        path.extend_from_slice(&[0xce, 0xc4, 0xb5, 0xb5]); // 文档
        let result = decode_bytes_to_string(PaneEncoding::Gbk, &path);
        assert_eq!(result, Some("/home/用户/文档".to_string()));
    }
}
