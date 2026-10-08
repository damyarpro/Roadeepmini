const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let b = part.get(1).copied().unwrap_or(0);
        let c = part.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(part[0] >> 2) as usize] as char);
        out.push(ALPHABET[(((part[0] & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if part.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if part.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
pub fn decode(text: &str, max: usize) -> Result<Vec<u8>, String> {
    if text.len() > max.div_ceil(3) * 4 || text.len() % 4 != 0 || !text.is_ascii() {
        return Err("local-audio-invalid".into());
    }
    let mut result = Vec::with_capacity(text.len() / 4 * 3);
    let chunks = text.as_bytes().chunks_exact(4);
    let total = chunks.len();
    for (index, c) in chunks.enumerate() {
        let val = |byte: u8| {
            ALPHABET
                .iter()
                .position(|x| *x == byte)
                .map(|n| n as u8)
                .ok_or("local-audio-invalid")
        };
        let a = val(c[0])?;
        let b = val(c[1])?;
        if c[2] == b'=' {
            if c[3] != b'=' || index + 1 != total || b & 15 != 0 {
                return Err("local-audio-invalid".into());
            }
            result.push(a << 2 | b >> 4);
        } else {
            let d = val(c[2])?;
            result.push(a << 2 | b >> 4);
            result.push(b << 4 | d >> 2);
            if c[3] == b'=' {
                if index + 1 != total || d & 3 != 0 {
                    return Err("local-audio-invalid".into());
                }
            } else {
                result.push(d << 6 | val(c[3])?);
            }
        }
        if result.len() > max {
            return Err("local-audio-invalid".into());
        }
    }
    Ok(result)
}
/// Validate RIFF chunks before any bytes reach a decoder process.
pub fn wav(bytes: &[u8], microphone: bool) -> Result<(), String> {
    let invalid = || "local-audio-invalid".to_string();
    if bytes.len() < 44
        || bytes.len() > 4 * 1024 * 1024
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
    {
        return Err(invalid());
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if u32_at(4) as usize + 8 != bytes.len() {
        return Err(invalid());
    }
    let mut offset = 12;
    let mut format = None;
    let mut pcm = None;
    while offset + 8 <= bytes.len() {
        let length = u32_at(offset + 4) as usize;
        let begin = offset + 8;
        let end = begin
            .checked_add(length)
            .filter(|n| *n <= bytes.len())
            .ok_or_else(invalid)?;
        match &bytes[offset..offset + 4] {
            b"fmt " => {
                if format.is_some() || length < 16 {
                    return Err(invalid());
                }
                let short = |at: usize| {
                    u16::from_le_bytes(bytes[begin + at..begin + at + 2].try_into().unwrap())
                };
                let rate = u32_at(begin + 4);
                if (microphone && rate != 16000) || !(8000..=48000).contains(&rate) {
                    return Err(invalid());
                }
                if short(0) != 1
                    || short(2) != 1
                    || short(14) != 16
                    || short(12) != 2
                    || u32_at(begin + 8) != rate * 2
                {
                    return Err(invalid());
                }
                format = Some(rate);
            }
            b"data" => {
                if pcm.is_some() || length == 0 || length % 2 != 0 {
                    return Err(invalid());
                }
                pcm = Some((begin, end));
            }
            _ => (),
        }
        offset = end.checked_add(length % 2).ok_or_else(invalid)?;
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    let rate = format.ok_or_else(invalid)? as usize;
    let (begin, end) = pcm.ok_or_else(invalid)?;
    let samples = (end - begin) / 2;
    if samples > rate * if microphone { 30 } else { 120 } || samples < rate / 10 {
        return Err(invalid());
    }
    if microphone {
        let energy: f64 = bytes[begin..end]
            .chunks_exact(2)
            .map(|c| {
                let sample = i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0;
                sample * sample
            })
            .sum();
        if (energy / samples as f64).sqrt() < 0.002 {
            return Err("local-audio-silence".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(samples: usize, value: i16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + samples as u32 * 2).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(16000u32.to_le_bytes());
        b.extend(32000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((samples as u32 * 2).to_le_bytes());
        for _ in 0..samples {
            b.extend(value.to_le_bytes());
        }
        b
    }
    #[test]
    fn base64_vectors_and_invalid() {
        for bytes in [b"".as_slice(), b"a", b"ab", b"abc", b"hello"] {
            assert_eq!(decode(&encode(bytes), 100).unwrap(), bytes);
        }
        for input in ["A", "!!!!", "A===", "Zh==", "Zg==Zg==", "Zm9="] {
            assert!(decode(input, 100).is_err());
        }
        assert!(decode("aGVsbG8=", 4).is_err());
    }
    #[test]
    fn audio_bounds_silence_and_corrupt() {
        assert!(wav(&fixture(1600, 1000), true).is_ok());
        assert_eq!(
            wav(&fixture(1600, 0), true).unwrap_err(),
            "local-audio-silence"
        );
        assert!(wav(&fixture(31 * 16000, 1000), true).is_err());
        let mut bad = fixture(1600, 1000);
        bad[22] = 2;
        assert!(wav(&bad, true).is_err());
        let mut bad = fixture(1600, 1000);
        bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(wav(&bad, false).is_err());
        let mut bad = fixture(1600, 1000);
        bad.truncate(43);
        assert!(wav(&bad, true).is_err());
    }
}
