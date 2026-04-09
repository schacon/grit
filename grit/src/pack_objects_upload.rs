//! Spawn `pack-objects` for upload-pack (hook or direct `grit`), write rev-list stdin, stream stdout.

use anyhow::{bail, Context, Result};
use grit_lib::config::ConfigSet;
use grit_lib::objects::ObjectId;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::grit_exe::grit_executable;

fn resolve_hook_path(git_dir: &Path, hook: &str) -> PathBuf {
    let p = Path::new(hook);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        git_dir.join(p)
    }
}

/// Build and spawn `git pack-objects` (via hook or `grit pack-objects`).
///
/// When `thin` is true, omit objects the client already has (requires matching `have` lines).
/// For a fetch/clone with no common objects, pass `false` so the pack is self-contained.
pub fn spawn_pack_objects_upload(git_dir: &Path, thin: bool) -> Result<Child> {
    let protected = ConfigSet::load_protected(true).unwrap_or_default();
    let hook_raw = protected.get("uploadpack.packobjectshook");
    let grit = grit_executable();

    let mut cmd = if let Some(ref hook_path) = hook_raw {
        let hook_resolved = resolve_hook_path(git_dir, hook_path);
        let mut c = Command::new("sh");
        c.arg("-c")
            .arg("exec \"$0\" \"$@\"")
            .arg(&hook_resolved)
            .arg("git")
            .arg("pack-objects")
            .arg("--revs");
        if thin {
            c.arg("--thin");
        }
        c.arg("--stdout")
            .arg("--progress")
            .arg("--delta-base-offset");
        c
    } else {
        let mut c = Command::new(&grit);
        c.arg("pack-objects").arg("--revs");
        if thin {
            c.arg("--thin");
        }
        c.arg("--stdout")
            .arg("--progress")
            .arg("--delta-base-offset");
        c
    };

    cmd.current_dir(git_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            if hook_raw.is_some() {
                anyhow::anyhow!("failed to spawn pack-objects hook")
            } else {
                anyhow::anyhow!("failed to spawn '{} pack-objects'", grit.display())
            }
        })
}

/// Write the stdin Git's `pack-objects --revs` expects (`--not` + have commit OIDs).
pub fn write_pack_objects_revs_stdin(
    pin: &mut impl Write,
    wants: &[ObjectId],
    have_commits: &[ObjectId],
) -> Result<()> {
    for w in wants {
        writeln!(pin, "{}", w.to_hex())?;
    }
    writeln!(pin, "--not")?;
    for h in have_commits {
        writeln!(pin, "{}", h.to_hex())?;
    }
    writeln!(pin)?;
    pin.flush()?;
    Ok(())
}

fn write_sideband_64k(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    const MAX_PAYLOAD: usize = 65515;
    for chunk in payload.chunks(MAX_PAYLOAD) {
        let len = 4 + 1 + chunk.len();
        write!(w, "{len:04x}")?;
        w.write_all(&[1u8])?;
        w.write_all(chunk)?;
    }
    Ok(())
}

/// Read pack bytes from `child` and write to `out`, optionally wrapping in side-band-64k (v0).
pub fn drain_pack_objects_child(
    mut child: Child,
    out: &mut impl Write,
    sideband: bool,
) -> Result<()> {
    let mut pack_out = child.stdout.take().context("pack-objects stdout")?;
    let stderr_child = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        if let Some(mut e) = stderr_child {
            let mut buf = Vec::new();
            let _ = e.read_to_end(&mut buf);
            buf
        } else {
            Vec::new()
        }
    });

    const CHUNK: usize = 32000;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = pack_out.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if sideband {
            write_sideband_64k(out, &buf[..n])?;
        } else {
            out.write_all(&buf[..n])?;
        }
    }

    let status = child.wait()?;
    let err_bytes = stderr_handle.join().unwrap_or_default();
    if !err_bytes.is_empty() {
        let _ = io::stderr().write_all(&err_bytes);
    }
    if !status.success() {
        bail!(
            "pack-objects failed with exit code {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
