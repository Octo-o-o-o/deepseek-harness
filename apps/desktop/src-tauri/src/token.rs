//! Per-launch desktop token. Never logged.

/// Encode raw bytes as lowercase hex.
///
/// # Parameters
/// - `bytes`: entropy from the platform RNG.
///
/// # Returns
/// A hex string twice as long as `bytes`.
pub fn encode_desktop_token(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        out.push_str(&format!("{byte:02x}"));
        out
    })
}

/// Mint a 32-byte hex token for one desktop launch.
///
/// # Returns
/// 64 hex characters, or an IO error when the platform RNG cannot be read.
pub fn generate_desktop_token() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(encode_desktop_token(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_each_byte() {
        assert_eq!(encode_desktop_token(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn generates_64_hex_chars() {
        let token = generate_desktop_token().unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
