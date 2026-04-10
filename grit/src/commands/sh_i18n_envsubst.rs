//! `grit sh-i18n--envsubst` — minimal envsubst for shell i18n fallbacks.
//!
//! Matches the interface of Git's `sh-i18n--envsubst` helper used by `git-sh-i18n.sh`
//! when GNU `gettext.sh` is unavailable.

use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::env;
use std::io::{self, Read, Write};

/// Run `grit sh-i18n--envsubst` with full argv (subcommand already stripped).
pub fn run_from_argv(rest: &[String]) -> Result<()> {
    match rest.len() {
        0 => bail!("we won't substitute all variables on stdin for you"),
        1 => {
            let template = &rest[0];
            let vars: Vec<String> = collect_variables(template).into_iter().collect();
            subst_stdin(&vars)?;
        }
        2 => {
            if rest[0] != "--variables" {
                bail!("first argument must be --variables when two are given");
            }
            for name in collect_variables(&rest[1]) {
                println!("{name}");
            }
        }
        _ => bail!("too many arguments"),
    }
    Ok(())
}

fn collect_variables(template: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        i += 1;
        let mut brace = false;
        if i < bytes.len() && bytes[i] == b'{' {
            brace = true;
            i += 1;
        }
        let name_start = i;
        let Some(&first) = bytes.get(i) else {
            break;
        };
        if !first.is_ascii_alphabetic() && first != b'_' {
            continue;
        }
        i += 1;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_alphanumeric() || c == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        if brace {
            if i < bytes.len() && bytes[i] == b'}' {
                let name = template[name_start..i].to_string();
                out.insert(name);
                i += 1;
            }
            // else: invalid `${` — skip; outer loop continues
        } else {
            out.insert(template[name_start..i].to_string());
        }
    }
    out
}

fn subst_stdin(allowed: &[String]) -> Result<()> {
    let mut stdin = io::stdin();
    let mut buf = Vec::new();
    stdin.read_to_end(&mut buf)?;
    let mut out = io::stdout();
    subst_bytes(&buf, allowed, &mut out)?;
    Ok(())
}

fn subst_bytes(input: &[u8], allowed: &[String], out: &mut impl Write) -> Result<()> {
    let mut i = 0usize;
    while i < input.len() {
        if input[i] != b'$' {
            out.write_all(&[input[i]])?;
            i += 1;
            continue;
        }

        let save = i;
        i += 1;
        let mut opening_brace = false;
        if i < input.len() && input[i] == b'{' {
            opening_brace = true;
            i += 1;
        }

        let name_start = i;
        let Some(&first) = input.get(i) else {
            out.write_all(b"$")?;
            break;
        };
        if !first.is_ascii_alphabetic() && first != b'_' {
            out.write_all(&input[save..=save])?;
            i = save + 1;
            continue;
        }
        i += 1;
        while i < input.len() {
            let c = input[i];
            if c.is_ascii_alphanumeric() || c == b'_' {
                i += 1;
            } else {
                break;
            }
        }

        let name_end = i;
        let valid = if opening_brace {
            if i < input.len() && input[i] == b'}' {
                i += 1;
                true
            } else {
                false
            }
        } else {
            true
        };

        let name_bytes = &input[name_start..name_end];
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            out.write_all(&input[save..i])?;
            continue;
        };

        if valid && allowed.binary_search(&name.to_string()).is_ok() {
            if let Ok(val) = env::var(name) {
                out.write_all(val.as_bytes())?;
            }
            continue;
        }

        out.write_all(&input[save..i])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_list() {
        let v: Vec<_> = collect_variables("$a $b ${a}").into_iter().collect();
        assert_eq!(v, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn subst_replaces_known_var() {
        let allowed = vec!["FOO".to_string()];
        env::remove_var("FOO");
        let mut dst = Vec::new();
        subst_bytes(b"pre $FOO post", &allowed, &mut dst).unwrap();
        assert_eq!(String::from_utf8(dst).unwrap(), "pre  post");
        env::set_var("FOO", "hello");
        let mut dst = Vec::new();
        subst_bytes(b"pre $FOO post", &allowed, &mut dst).unwrap();
        assert_eq!(String::from_utf8(dst).unwrap(), "pre hello post");
        env::remove_var("FOO");
    }
}
