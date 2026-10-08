//! Small bounded base64 wire codec; no media bytes are interpolated into commands.
const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        out.push(ABC[(c[0] >> 2) as usize] as char);
        out.push(ABC[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        out.push(if c.len() > 1 { ABC[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { ABC[(c[2] & 63) as usize] as char } else { '=' });
    }
    out
}
#[cfg(test)]
mod tests {
    #[test] fn vectors() {
        for (input, expected) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("hello", "aGVsbG8=")] {
            assert_eq!(super::encode(input.as_bytes()), expected);
        }
    }
}
