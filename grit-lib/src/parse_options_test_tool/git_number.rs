//! `git_parse_signed` / `git_parse_unsigned` compatible parsing (k/m/g suffixes).

fn unit_factor(end: &str) -> Option<u128> {
    if end.is_empty() {
        return Some(1);
    }
    if end.eq_ignore_ascii_case("k") {
        return Some(1024);
    }
    if end.eq_ignore_ascii_case("m") {
        return Some(1024 * 1024);
    }
    if end.eq_ignore_ascii_case("g") {
        return Some(1024_u128 * 1024 * 1024);
    }
    None
}

/// Parse signed integer with optional k/m/g suffix, matching `git_parse_signed`.
pub fn git_parse_signed(value: &str, max: i128) -> Result<i128, std::io::ErrorKind> {
    if value.is_empty() {
        return Err(std::io::ErrorKind::InvalidInput);
    }
    let mut end_idx = 0usize;
    let bytes = value.as_bytes();
    if bytes.first() == Some(&b'+') || bytes.first() == Some(&b'-') {
        end_idx = 1;
    }
    while end_idx < bytes.len() && bytes[end_idx].is_ascii_digit() {
        end_idx += 1;
    }
    let num_part = &value[..end_idx];
    let suffix = &value[end_idx..];
    let val: i128 = num_part
        .parse::<i128>()
        .map_err(|_| std::io::ErrorKind::InvalidInput)?;
    let Some(factor) = unit_factor(suffix) else {
        return Err(std::io::ErrorKind::InvalidInput);
    };
    let factor_i = factor as i128;
    if val < 0 && (-max - 1) / factor_i > val || val > 0 && max / factor_i < val {
        return Err(std::io::ErrorKind::InvalidData);
    }
    Ok(val * factor_i)
}

/// Parse unsigned integer with optional k/m/g suffix, matching `git_parse_unsigned`.
pub fn git_parse_unsigned(value: &str, max: u128) -> Result<u128, std::io::ErrorKind> {
    if value.is_empty() || value.contains('-') {
        return Err(std::io::ErrorKind::InvalidInput);
    }
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let num_part = &value[..i];
    let suffix = &value[i..];
    let val: u128 = num_part
        .parse::<u128>()
        .map_err(|_| std::io::ErrorKind::InvalidInput)?;
    let Some(factor) = unit_factor(suffix) else {
        return Err(std::io::ErrorKind::InvalidInput);
    };
    val.checked_mul(factor)
        .filter(|&v| v <= max)
        .ok_or(std::io::ErrorKind::InvalidData)
}
