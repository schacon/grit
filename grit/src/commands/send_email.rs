//! `grit send-email` — send a collection of patches as emails.
//!
//! Minimal implementation that supports sending patches via an
//! SMTP server command (--smtp-server) or the sendmail interface.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for `grit send-email`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send a collection of patches as emails")]
pub struct Args {
    /// Patch files or revision range to send.
    #[arg(value_name = "patch|rev")]
    pub patches: Vec<String>,

    /// Sender email address.
    #[arg(long)]
    pub from: Option<String>,

    /// Recipient email address(es).
    #[arg(long)]
    pub to: Vec<String>,

    /// CC recipient(s).
    #[arg(long)]
    pub cc: Vec<String>,

    /// BCC recipient(s).
    #[arg(long)]
    pub bcc: Vec<String>,

    /// Subject for compose mode.
    #[arg(long)]
    pub subject: Option<String>,

    /// In-Reply-To Message-ID.
    #[arg(long = "in-reply-to")]
    pub in_reply_to: Option<String>,

    /// SMTP server command or host.
    #[arg(long = "smtp-server")]
    pub smtp_server: Option<String>,

    /// SMTP server port.
    #[arg(long = "smtp-server-port")]
    pub smtp_server_port: Option<u16>,

    /// SMTP encryption (ssl, tls).
    #[arg(long = "smtp-encryption")]
    pub smtp_encryption: Option<String>,

    /// SMTP username.
    #[arg(long = "smtp-user")]
    pub smtp_user: Option<String>,

    /// SMTP password.
    #[arg(long = "smtp-pass")]
    pub smtp_pass: Option<String>,

    /// Suppress certain auto-cc behavior.
    #[arg(long = "suppress-cc")]
    pub suppress_cc: Option<String>,

    /// Confirmation mode (auto, always, never, compose).
    #[arg(long)]
    pub confirm: Option<String>,

    /// Compose a message (opens editor or uses subject/body).
    #[arg(long)]
    pub compose: bool,

    /// Force 8-bit encoding with specified charset.
    #[arg(long = "8bit-encoding")]
    pub eight_bit_encoding: Option<String>,

    /// Dry-run mode.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Run a command for each To: address.
    #[arg(long = "to-cmd")]
    pub to_cmd: Option<String>,

    /// Run a command for each Cc: address.
    #[arg(long = "cc-cmd")]
    pub cc_cmd: Option<String>,

    /// Header command to generate extra headers.
    #[arg(long = "header-cmd")]
    pub header_cmd: Option<String>,

    /// Envelope sender address.
    #[arg(long = "envelope-sender")]
    pub envelope_sender: Option<String>,

    /// Use --no-thread to disable threading.
    #[arg(long = "no-thread")]
    pub no_thread: bool,

    /// Thread style (shallow, deep).
    #[arg(long = "thread")]
    pub thread: Option<String>,

    /// Hook validation (sendmail-validate).
    #[arg(long = "validate")]
    pub validate: bool,

    /// Skip hook validation.
    #[arg(long = "no-validate")]
    pub no_validate: bool,

    /// Add extra header lines.
    #[arg(long = "transfer-encoding")]
    pub transfer_encoding: Option<String>,

    /// Number of patches to send (cover letter related).
    #[arg(short = 'v', long = "reroll-count")]
    pub reroll_count: Option<String>,

    /// Force sending.
    #[arg(long)]
    pub force: bool,

    /// Format-patch options passed through (e.g. --cover-letter).
    #[arg(long = "cover-letter")]
    pub cover_letter: bool,

    /// How to fill the cover letter body (message, subject, auto, none).
    #[arg(long = "cover-from-description")]
    pub cover_from_description: Option<String>,

    /// Annotate patches.
    #[arg(long)]
    pub annotate: bool,

    /// Quiet mode.
    #[arg(short, long)]
    pub quiet: bool,

    /// Batch size.
    #[arg(long = "batch-size")]
    pub batch_size: Option<usize>,

    /// Identity to use for sendemail config.
    #[arg(long)]
    pub identity: Option<String>,
}

pub fn run(args: Args) -> Result<()> {
    let repo = grit_lib::repo::Repository::discover(None).context("not a git repository")?;

    let config = grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true)?;

    // Resolve from address
    let from = args
        .from
        .clone()
        .or_else(|| {
            config.get("sendemail.from").or_else(|| {
                let name = config
                    .get("user.name")
                    .unwrap_or_else(|| "User".to_string());
                let email = config
                    .get("user.email")
                    .unwrap_or_else(|| "user@example.com".to_string());
                Some(format!("{} <{}>", name, email))
            })
        })
        .unwrap_or_else(|| "user@example.com".to_string());

    // Resolve SMTP server
    let smtp_server = args
        .smtp_server
        .clone()
        .or_else(|| config.get("sendemail.smtpserver"));

    // Resolve confirm mode
    let confirm = args
        .confirm
        .clone()
        .or_else(|| config.get("sendemail.confirm"))
        .unwrap_or_else(|| "auto".to_string());

    // Collect patches
    let patches = collect_patches(&args, &repo)?;

    if patches.is_empty() {
        bail!("No patches to send");
    }

    // Resolve to-cmd recipients
    let mut to_addrs = args.to.clone();
    if let Some(ref cmd) = args.to_cmd {
        for patch in &patches {
            let output = Command::new("sh")
                .args(["-c", &format!("{} < {}", cmd, patch.display())])
                .output()
                .context("failed to run --to-cmd")?;
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let addr = line.trim().to_string();
                    if !addr.is_empty() && !to_addrs.contains(&addr) {
                        to_addrs.push(addr);
                    }
                }
            }
        }
    }

    // Resolve cc-cmd recipients
    let mut cc_addrs = args.cc.clone();
    if let Some(ref cmd) = args.cc_cmd {
        for patch in &patches {
            let output = Command::new("sh")
                .args(["-c", &format!("{} < {}", cmd, patch.display())])
                .output()
                .context("failed to run --cc-cmd")?;
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let addr = line.trim().to_string();
                    if !addr.is_empty() && !cc_addrs.contains(&addr) {
                        cc_addrs.push(addr);
                    }
                }
            }
        }
    }

    // Validate hook
    if args.validate && !args.no_validate {
        let hook_path = repo.git_dir.join("hooks").join("sendemail-validate");
        if hook_path.exists() {
            for patch in &patches {
                let status = Command::new(&hook_path)
                    .arg(patch)
                    .status()
                    .context("failed to run sendemail-validate hook")?;
                if !status.success() {
                    bail!("sendemail-validate hook rejected '{}'", patch.display());
                }
            }
        }
    }

    // Send each patch
    for (i, patch) in patches.iter().enumerate() {
        let content = fs::read_to_string(patch)
            .with_context(|| format!("failed to read '{}'", patch.display()))?;

        // Parse headers from patch
        let (headers, body) = parse_patch_email(&content);

        // Build email
        let mut email = String::new();
        email.push_str(&format!("From: {}\n", from));
        for to in &to_addrs {
            email.push_str(&format!("To: {}\n", to));
        }
        for cc in &cc_addrs {
            email.push_str(&format!("Cc: {}\n", cc));
        }
        if let Some(ref env_sender) = args.envelope_sender {
            email.push_str(&format!("Envelope-Sender: {}\n", env_sender));
        }
        if let Some(ref irt) = args.in_reply_to {
            email.push_str(&format!("In-Reply-To: {}\n", irt));
        }

        // Include original headers
        for (key, value) in &headers {
            let lk = key.to_lowercase();
            if lk != "from" && lk != "to" && lk != "cc" {
                email.push_str(&format!("{}: {}\n", key, value));
            }
        }

        // 8-bit encoding
        if let Some(ref enc) = args.eight_bit_encoding {
            email.push_str(&format!("Content-Type: text/plain; charset={}\n", enc));
            email.push_str("Content-Transfer-Encoding: 8bit\n");
        }

        email.push('\n');
        email.push_str(&body);

        if args.dry_run {
            if !args.quiet {
                eprintln!("Dry-run: would send {}", patch.display());
            }
            print!("{}", email);
            continue;
        }

        // Send via smtp-server command or sendmail
        if let Some(ref server) = smtp_server {
            let server_path = Path::new(server);
            if server_path.exists() && server_path.is_file() {
                // It's a sendmail-like program
                let mut cmd_args: Vec<String> = Vec::new();

                // Add -f for envelope sender
                if let Some(ref env_sender) = args.envelope_sender {
                    cmd_args.push("-f".to_string());
                    cmd_args.push(env_sender.clone());
                }

                // Add recipients
                for to in &to_addrs {
                    cmd_args.push(to.clone());
                }
                for cc in &cc_addrs {
                    cmd_args.push(cc.clone());
                }
                for bcc in &args.bcc {
                    cmd_args.push(bcc.clone());
                }

                let mut child = Command::new(server)
                    .args(&cmd_args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .with_context(|| format!("failed to spawn '{}'", server))?;

                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(email.as_bytes())?;
                }

                let output = child.wait_with_output()?;
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    bail!("smtp-server failed: {}", err);
                }
            } else {
                // It's an SMTP host — not yet implemented
                bail!(
                    "SMTP host '{}' sending not yet implemented; use a sendmail-compatible command",
                    server
                );
            }
        } else {
            // Try /usr/sbin/sendmail
            let sendmail = "/usr/sbin/sendmail";
            if Path::new(sendmail).exists() {
                let mut child = Command::new(sendmail)
                    .args(["-t", "-oi"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .context("failed to spawn sendmail")?;

                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(email.as_bytes())?;
                }

                let status = child.wait()?;
                if !status.success() {
                    bail!("sendmail failed");
                }
            } else {
                bail!("No SMTP server configured. Use --smtp-server or set sendemail.smtpserver");
            }
        }

        if !args.quiet {
            eprintln!("OK. Log says:");
            if i == 0 && confirm != "never" {
                // First patch
            }
        }
    }

    Ok(())
}

fn collect_patches(args: &Args, repo: &grit_lib::repo::Repository) -> Result<Vec<PathBuf>> {
    let mut patches = Vec::new();

    for p in &args.patches {
        let path = PathBuf::from(p);
        if path.exists() && path.is_file() {
            patches.push(path);
        } else if path.is_dir() {
            // Collect .patch files from directory
            if let Ok(entries) = fs::read_dir(&path) {
                let mut files: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|e| e == "patch").unwrap_or(false))
                    .collect();
                files.sort();
                patches.extend(files);
            }
        } else {
            // Treat as revision range — generate patches
            let git = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("git"));
            let output = Command::new(&git)
                .args(["format-patch", "--stdout", p])
                .current_dir(repo.work_tree.as_deref().unwrap_or(&repo.git_dir))
                .output()
                .context("failed to run format-patch")?;

            if output.status.success() {
                // Write to temp files
                let tmp_dir = std::env::temp_dir().join("grit-send-email");
                fs::create_dir_all(&tmp_dir)?;
                let content = String::from_utf8_lossy(&output.stdout);

                // Split on "From " boundaries
                let mut patch_num = 0;
                let mut current = String::new();
                for line in content.lines() {
                    if line.starts_with("From ") && !current.is_empty() {
                        patch_num += 1;
                        let path = tmp_dir.join(format!("{:04}.patch", patch_num));
                        fs::write(&path, &current)?;
                        patches.push(path);
                        current.clear();
                    }
                    current.push_str(line);
                    current.push('\n');
                }
                if !current.is_empty() {
                    patch_num += 1;
                    let path = tmp_dir.join(format!("{:04}.patch", patch_num));
                    fs::write(&path, &current)?;
                    patches.push(path);
                }
            } else {
                bail!(
                    "format-patch failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    Ok(patches)
}

fn parse_patch_email(content: &str) -> (Vec<(String, String)>, String) {
    let mut headers = Vec::new();
    let mut body = String::new();
    let mut in_headers = true;

    for line in content.lines() {
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                continue;
            }
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim().to_string();
                let value = line[colon_pos + 1..].trim().to_string();
                headers.push((key, value));
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }

    (headers, body)
}
