//! Base-85 codec for `GIT binary patch` sections (matches `git/base85.c`).

const EN85: &[u8; 85] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

fn prep_decode_table() -> [u8; 256] {
    let mut de85 = [0u8; 256];
    for (i, &c) in EN85.iter().enumerate() {
        de85[c as usize] = (i + 1) as u8;
    }
    de85
}

/// Encode bytes using Git's base-85 alphabet (matches `encode_85` in `git/base85.c`).
pub fn encode(mut data: &[u8]) -> String {
    let mut result = String::new();
    while !data.is_empty() {
        let mut acc: u32 = 0;
        let mut cnt = 24i32;
        while cnt >= 0 {
            let ch = u32::from(data[0]);
            data = &data[1..];
            acc |= ch << cnt;
            if data.is_empty() {
                break;
            }
            cnt -= 8;
        }
        let mut buf = [0u8; 5];
        for i in (0..5).rev() {
            let val = (acc % 85) as usize;
            acc /= 85;
            buf[i] = EN85[val];
        }
        result.push_str(std::str::from_utf8(&buf).expect("EN85 is ASCII"));
    }
    result
}

/// Decode the base-85 body of one binary patch line (after the length prefix).
///
/// `out_len` is the number of raw (compressed) bytes this line contributes.
pub fn decode_body(buffer: &[u8], mut out_len: usize) -> anyhow::Result<Vec<u8>> {
    static DE85: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    let de85 = DE85.get_or_init(prep_decode_table);

    let mut dst = Vec::with_capacity(out_len);
    let mut pos = 0usize;

    while out_len > 0 {
        let mut acc: u32 = 0;
        for _ in 0..4 {
            let ch = *buffer
                .get(pos)
                .ok_or_else(|| anyhow::anyhow!("truncated base85 line"))?;
            pos += 1;
            let de = de85[ch as usize];
            if de == 0 {
                anyhow::bail!("invalid base85 alphabet byte {ch}");
            }
            acc = acc
                .checked_mul(85)
                .and_then(|a| a.checked_add(u32::from(de - 1)))
                .ok_or_else(|| anyhow::anyhow!("base85 overflow"))?;
        }
        let ch = *buffer
            .get(pos)
            .ok_or_else(|| anyhow::anyhow!("truncated base85 line"))?;
        pos += 1;
        let de = de85[ch as usize];
        if de == 0 {
            anyhow::bail!("invalid base85 alphabet byte {ch}");
        }
        acc = acc
            .checked_mul(85)
            .and_then(|a| a.checked_add(u32::from(de - 1)))
            .ok_or_else(|| anyhow::anyhow!("base85 overflow"))?;

        let chunk = out_len.min(4);
        out_len -= chunk;
        let mut a = acc;
        for _ in 0..chunk {
            a = a.rotate_left(8);
            dst.push(a as u8);
        }
    }

    if pos != buffer.len() {
        anyhow::bail!("trailing base85 data");
    }
    Ok(dst)
}
