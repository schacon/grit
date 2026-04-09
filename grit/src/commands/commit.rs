//! `grit commit` — record changes to the repository.
//!
//! Creates a new commit object from the current index state, updates HEAD
//! to point to the new commit, and optionally runs hooks.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::{
    diff_index_to_tree, diff_index_to_worktree, status_apply_rename_copy_detection, DiffEntry,
    DiffStatus,
};
use grit_lib::error::Error;
use grit_lib::hooks::{run_hook, HookResult};
use grit_lib::index::Index;
use grit_lib::objects::{serialize_commit, CommitData, ObjectId, ObjectKind};
use grit_lib::refs::append_reflog;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use grit_lib::state::{resolve_head, HeadState};
use grit_lib::write_tree::{
    write_tree_from_index, write_tree_from_index_subset, write_tree_partial_from_index,
};

use crate::ident::{resolve_email, resolve_name, IdentRole};

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use time::OffsetDateTime;

/// Arguments for `grit commit`.
#[derive(Debug, ClapArgs)]
#[command(about = "Record changes to the repository")]
pub struct Args {
    /// Use the given message as the commit message.
    #[arg(short = 'm', long = "message")]
    pub message: Vec<String>,

    /// Take the commit message from the given file.
    #[arg(short = 'F', long = "file")]
    pub file: Option<String>,

    /// Commit all changed tracked files (like `git add -u` first).
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Amend the last commit.
    #[arg(long = "amend")]
    pub amend: bool,

    /// Allow an empty commit (no changes).
    #[arg(long = "allow-empty")]
    pub allow_empty: bool,

    /// Allow an empty commit message.
    #[arg(long = "allow-empty-message")]
    pub allow_empty_message: bool,

    /// Show what would be committed without committing.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Suppress commit summary output.
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Add Signed-off-by trailer.
    #[arg(short = 's', long = "signoff")]
    pub signoff: bool,

    /// Take the commit message from an existing commit.
    #[arg(short = 'C', long = "reuse-message", value_name = "COMMIT")]
    pub reuse_message: Option<String>,

    /// Like -C, but open editor to modify the message.
    #[arg(short = 'c', long = "reedit-message", value_name = "COMMIT")]
    pub reedit_message: Option<String>,

    /// Override the author.
    #[arg(long = "author")]
    pub author: Option<String>,

    /// Override the date.
    #[arg(long = "date")]
    pub date: Option<String>,

    /// Suppress the post-rewrite hook.
    #[arg(long = "no-post-rewrite")]
    pub no_post_rewrite: bool,

    /// Give output in short format (for dry-run).
    #[arg(long = "short")]
    pub short: bool,

    /// Give output in porcelain format (for dry-run).
    #[arg(long = "porcelain")]
    pub porcelain: bool,

    /// Give output in long format (default for dry-run).
    #[arg(long = "long")]
    pub long: bool,

    /// Include staged changes when given pathspec (with -i).
    #[arg(short = 'i', long = "include")]
    pub include: bool,

    /// Only commit specified paths (with -o or --only).
    #[arg(short = 'o', long = "only")]
    pub only: bool,

    /// Interactively add changes.
    #[arg(long = "interactive")]
    pub interactive: bool,

    /// Select hunks interactively (accepted for Git compatibility; not implemented).
    #[arg(short = 'p', long = "patch", hide = true)]
    pub patch: bool,

    /// Untracked files mode.
    #[arg(short = 'u', long = "untracked-files", value_name = "MODE", num_args = 0..=1, default_missing_value = "all")]
    pub untracked_files: Option<String>,

    /// Verbose - show diff in commit message editor.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress verbose output.
    #[arg(long = "no-verbose")]
    pub no_verbose: bool,

    /// Override cleanup mode.
    #[arg(long = "cleanup", value_name = "MODE")]
    pub cleanup: Option<String>,

    /// Use specified template file.
    #[arg(short = 't', long = "template", value_name = "FILE")]
    pub template: Option<String>,

    /// Edit the commit message (used with -C).
    #[arg(short = 'e', long = "edit")]
    pub edit: bool,

    /// Suppress editing the commit message.
    #[arg(long = "no-edit")]
    pub no_edit: bool,

    /// Set the commit status (accepted but not used).
    #[arg(long = "status")]
    pub status: bool,

    /// Suppress commit status in editor template.
    #[arg(long = "no-status")]
    pub no_status: bool,

    /// Add a Signed-off-by trailer with specific value.
    #[arg(long = "trailer", value_name = "TOKEN:VALUE")]
    pub trailer: Vec<String>,

    /// Override gpg sign.
    #[arg(short = 'S', long = "gpg-sign", value_name = "KEYID", num_args = 0..=1, default_missing_value = "")]
    pub gpg_sign: Option<String>,

    /// Don't sign the commit.
    #[arg(long = "no-gpg-sign")]
    pub no_gpg_sign: bool,

    /// Don't verify the commit message.
    #[arg(long = "no-verify", short = 'n')]
    pub no_verify: bool,

    /// Fixup commit.
    #[arg(long = "fixup", value_name = "COMMIT")]
    pub fixup: Option<String>,

    /// Squash commit.
    #[arg(long = "squash", value_name = "COMMIT")]
    pub squash: Option<String>,

    /// Reset author.
    #[arg(long = "reset-author")]
    pub reset_author: bool,

    /// Read pathspecs from a file (use `-` for stdin), same rules as `git add`.
    #[arg(long = "pathspec-from-file", value_name = "FILE")]
    pub pathspec_from_file: Option<String>,

    /// NUL-separated entries for `--pathspec-from-file` (C-quoting not allowed).
    #[arg(long = "pathspec-file-nul")]
    pub pathspec_file_nul: bool,

    /// Pathspec — files to include in the commit (stages them first).
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub pathspec: Vec<String>,
}

/// Parsed `--fixup` value: plain autosquash vs `amend:` / `reword:` forms.
#[derive(Debug, Clone)]
enum FixupMode {
    /// `fixup! <subject>` one-liner (or `-m` append); uses editor only with `--edit`.
    Fixup,
    /// `amend!` / `reword!` message body built from the target commit.
    AmendStyle { is_reword: bool },
}

#[derive(Debug, Clone)]
struct FixupParsed {
    mode: FixupMode,
    commit_ref: String,
}

/// Run the `commit` command.
pub fn run(mut args: Args) -> Result<()> {
    // Tests and some scripts pass `-q` after `-m MSG`; if it lands in the
    // trailing pathspec bucket, strip it so we match Git (quiet is already
    // handled by the top-level flag).
    while args
        .pathspec
        .last()
        .is_some_and(|s| s == "-q" || s == "--quiet")
    {
        args.pathspec.pop();
    }

    if args.pathspec_file_nul && args.pathspec_from_file.is_none() {
        bail!("fatal: the option '--pathspec-file-nul' requires '--pathspec-from-file'");
    }

    if let Some(ref psf) = args.pathspec_from_file {
        if args.interactive || args.patch {
            bail!(
                "fatal: options '--pathspec-from-file' and '--interactive/--patch' cannot be used together"
            );
        }
        if args.all {
            bail!("fatal: options '--pathspec-from-file' and '-a' cannot be used together");
        }
        if !args.pathspec.is_empty() {
            bail!("fatal: '--pathspec-from-file' and pathspec arguments cannot be used together");
        }
        let data = if psf == "-" {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .context("reading pathspecs from stdin")?;
            buf
        } else {
            fs::read(psf).with_context(|| format!("cannot read pathspec file '{psf}'"))?
        };
        args.pathspec =
            crate::pathspec::parse_pathspecs_from_source(&data, args.pathspec_file_nul)?;
    }

    // Validate conflicting options
    let msg_source_count = [
        !args.message.is_empty(),
        args.file.is_some(),
        args.reuse_message.is_some(),
        args.reedit_message.is_some(),
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if msg_source_count > 1 {
        bail!("Only one of -m, -F, -C, -c can be used.");
    }

    if args.reset_author && args.author.is_some() {
        bail!("options '--reset-author' and '--author' cannot be used together");
    }

    // -a and explicit pathspec don't mix
    if args.all && !args.pathspec.is_empty() {
        bail!(
            "paths '{}' with -a does not make sense",
            args.pathspec.join(" ")
        );
    }

    // --include and --only don't mix
    if args.include && args.only {
        bail!("--include and --only are mutually exclusive");
    }

    if args.fixup.is_some() && args.squash.is_some() {
        bail!("fatal: options '--squash' and '--fixup' cannot be used together");
    }

    let fixup_parsed: Option<FixupParsed> = if let Some(ref raw) = args.fixup {
        Some(parse_fixup_argument(raw)?)
    } else {
        None
    };

    if let Some(ref fp) = fixup_parsed {
        match &fp.mode {
            FixupMode::AmendStyle { is_reword: true } => {
                if !args.message.is_empty() {
                    bail!("fatal: options '-m' and '--fixup:reword' cannot be used together");
                }
            }
            FixupMode::AmendStyle { is_reword: false } => {
                if !args.message.is_empty() {
                    bail!("fatal: options '-m' and '--fixup:amend' cannot be used together");
                }
            }
            FixupMode::Fixup => {}
        }
    }

    if fixup_parsed
        .as_ref()
        .is_some_and(|f| matches!(f.mode, FixupMode::AmendStyle { is_reword: true }))
        && (args.all
            || args.include
            || args.only
            || args.interactive
            || args.patch
            || !args.pathspec.is_empty())
    {
        if !args.pathspec.is_empty() {
            let p = &args.pathspec[0];
            bail!("fatal: reword option of '--fixup' and path '{p}' cannot be used together");
        }
        bail!("fatal: reword option of '--fixup' and '--patch/--interactive/--all/--include/--only' cannot be used together");
    }

    if fixup_parsed.is_some() {
        if args.reuse_message.is_some() {
            bail!("fatal: options '-C' and '--fixup' cannot be used together");
        }
        if args.reedit_message.is_some() {
            bail!("fatal: options '-c' and '--fixup' cannot be used together");
        }
        if args.file.is_some() {
            bail!("fatal: options '-F' and '--fixup' cannot be used together");
        }
    }

    let fixup_amend_style = fixup_parsed
        .as_ref()
        .is_some_and(|f| matches!(f.mode, FixupMode::AmendStyle { .. }));
    if args.pathspec.is_empty()
        && (args.include
            || (args.only
                && !args.allow_empty
                && (!args.amend || (fixup_parsed.is_some() && !fixup_amend_style))))
    {
        bail!("fatal: No paths with --include/--only does not make sense.");
    }

    let repo = Repository::discover(None).context("not a git repository")?;

    let reset_author_allowed = args.amend
        || args.reuse_message.is_some()
        || args.reedit_message.is_some()
        || repo.git_dir.join("CHERRY_PICK_HEAD").exists()
        || repo.git_dir.join("REBASE_HEAD").exists();
    if args.reset_author && !reset_author_allowed {
        bail!("--reset-author can be used only with -C, -c or --amend.");
    }

    let work_tree = repo.work_tree.as_deref();

    // If -a, stage all tracked file changes first
    if args.all {
        if let Some(wt) = work_tree {
            auto_stage_tracked(&repo, wt)?;
        }
    }

    // If pathspec given, stage those specific files first (returns paths included in this commit)
    let pathspec_matched: Option<HashSet<Vec<u8>>> = if !args.pathspec.is_empty() {
        let Some(wt) = work_tree else {
            bail!("pathspec requires a work tree");
        };
        Some(stage_pathspec_files(&repo, wt, &args.pathspec)?)
    } else {
        None
    };

    let index_path = resolved_index_path(&repo);

    // Load index
    let index = match repo.load_index_at(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    // Resolve HEAD for parent(s) and optional base tree for partial commits
    let head = resolve_head(&repo.git_dir)?;
    let parent_tree_oid = if let Some(head_oid) = head.oid() {
        let obj = repo.odb.read(head_oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        Some(commit.tree)
    } else {
        None
    };

    // Write tree: pathspec commits record only matched paths (Git partial / initial pathspec commit)
    let tree_oid = match (&pathspec_matched, &parent_tree_oid) {
        (Some(paths), Some(base)) if !paths.is_empty() => {
            match write_tree_partial_from_index(&repo.odb, &index, base, paths) {
                Ok(oid) => oid,
                Err(err) => {
                    if is_permission_denied_error(&err) {
                        eprintln!(
                            "error: insufficient permission for adding an object to repository database .git/objects"
                        );
                        eprintln!("error: Error building trees");
                        std::process::exit(128);
                    }
                    return Err(err.into());
                }
            }
        }
        (Some(paths), None) if !paths.is_empty() => {
            match write_tree_from_index_subset(&repo.odb, &index, paths) {
                Ok(oid) => oid,
                Err(err) => {
                    if is_permission_denied_error(&err) {
                        eprintln!(
                            "error: insufficient permission for adding an object to repository database .git/objects"
                        );
                        eprintln!("error: Error building trees");
                        std::process::exit(128);
                    }
                    return Err(err.into());
                }
            }
        }
        _ => match write_tree_from_index(&repo.odb, &index, "") {
            Ok(oid) => oid,
            Err(err) => {
                if is_permission_denied_error(&err) {
                    eprintln!(
                        "error: insufficient permission for adding an object to repository database .git/objects"
                    );
                    eprintln!("error: Error building trees");
                    std::process::exit(128);
                }
                return Err(err.into());
            }
        },
    };
    let mut parents = Vec::new();
    let old_head_oid = head.oid().cloned();

    if args.amend {
        // Amend: use the parent(s) of the current HEAD commit
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            parents = commit.parents;
        }
    } else {
        if let Some(head_oid) = head.oid() {
            parents.push(*head_oid);
        }

        // Check for MERGE_HEAD
        let merge_heads = grit_lib::state::read_merge_heads(&repo.git_dir)?;
        parents.extend(merge_heads);
    }

    let head_tree = match head.oid() {
        Some(oid) => {
            let obj = repo.odb.read(oid)?;
            let c = grit_lib::objects::parse_commit(&obj.data)?;
            Some(c.tree)
        }
        None => None,
    };

    let skip_index_tree_vs_parent = fixup_parsed
        .as_ref()
        .is_some_and(|f| matches!(f.mode, FixupMode::AmendStyle { .. }));

    // `--fixup=reword:` and `--fixup=amend: --only` record a new commit with the same tree as
    // `HEAD` while leaving the index (and staged changes) untouched — matching Git's behavior
    // for autosquash helper commits.
    let mut tree_oid = tree_oid;
    if let Some(ref fp) = fixup_parsed {
        if matches!(fp.mode, FixupMode::AmendStyle { is_reword: true })
            || (matches!(fp.mode, FixupMode::AmendStyle { is_reword: false }) && args.only)
        {
            let Some(t) = head_tree else {
                bail!("nothing to commit");
            };
            tree_oid = t;
        }
    }

    // For initial commits with empty tree (only ITA entries), fail
    if !args.allow_empty && parents.is_empty() {
        let empty_tree =
            grit_lib::objects::ObjectId::from_hex("4b825dc642cb6eb9a060e54bf8d69288fbee4904")
                .unwrap_or(tree_oid);
        if tree_oid == empty_tree {
            bail!("nothing to commit");
        }
    }

    let config = ConfigSet::load(Some(&repo.git_dir), true)?;

    let staged = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;
    let unstaged_raw = if let Some(wt) = work_tree {
        diff_index_to_worktree(&repo.odb, &index, wt)?
    } else {
        Vec::new()
    };
    let (rename_threshold, rename_copies) = commit_rename_settings(&config);
    let unstaged = if let Some(th) = rename_threshold {
        status_apply_rename_copy_detection(
            &repo.odb,
            unstaged_raw,
            th,
            rename_copies,
            head_tree.as_ref(),
        )?
    } else {
        unstaged_raw
    };
    let untracked = if let Some(wt) = work_tree {
        find_untracked_files(wt, &index)?
    } else {
        Vec::new()
    };

    if !args.allow_empty
        && !args.amend
        && !skip_index_tree_vs_parent
        && staged.is_empty()
        && !parents.is_empty()
    {
        let parent_obj = repo.odb.read(&parents[0])?;
        let parent_commit = grit_lib::objects::parse_commit(&parent_obj.data)?;
        if parent_commit.tree == tree_oid {
            if work_tree.is_some() {
                if !unstaged.is_empty() {
                    println!(
                        "no changes added to commit (use \"git add\" and/or \"git commit -a\")"
                    );
                } else if !untracked.is_empty() {
                    println!(
                        "nothing added to commit but untracked files present (use \"git add\" to track)"
                    );
                }
            }
            bail!("nothing to commit, working tree clean");
        }
    }

    // --dry-run: show what would be committed and exit
    if args.dry_run {
        print_dry_run(&head, &staged, &unstaged, &untracked)?;
        return Ok(());
    }

    // Run pre-commit hook
    if let HookResult::Failed(code) = run_hook(&repo, "pre-commit", &[], None) {
        bail!("pre-commit hook exited with status {code}");
    }

    let template_path = resolve_commit_template_path(&args, &config)?;
    let use_editor_for_message = commit_uses_editor(&args, fixup_parsed.as_ref());

    let msg_result = prepare_commit_message(
        &args,
        &repo,
        &config,
        fixup_parsed.as_ref(),
        template_path.as_deref(),
        use_editor_for_message,
        &head,
    )?;
    let mut message = normalize_autosquash_editor_message(
        &args,
        fixup_parsed.as_ref(),
        use_editor_for_message,
        &msg_result.message,
    );
    let mut raw_message = msg_result.raw_bytes;
    let template_for_aborted_check = template_path.filter(|_| use_editor_for_message);

    if message.trim().is_empty() && !args.allow_empty_message {
        eprintln!("Aborting commit due to empty commit message.");
        std::process::exit(1);
    }

    if let Some(ref tpl) = template_for_aborted_check {
        if template_untouched(&message, tpl) && !args.allow_empty_message {
            eprintln!("Aborting commit; you did not edit the message.");
            std::process::exit(1);
        }
    }

    if fixup_parsed.as_ref().is_some_and(|f| {
        matches!(f.mode, FixupMode::AmendStyle { .. }) && message.starts_with("amend! ")
    }) && !args.allow_empty_message
    {
        let body = message_after_first_line(&message);
        if body.trim().is_empty() {
            eprintln!("Aborting commit due to empty commit message body.");
            std::process::exit(1);
        }
    }

    // Check i18n.commitEncoding for non-UTF-8 commit messages
    let commit_encoding = config
        .get("i18n.commitEncoding")
        .or_else(|| config.get("i18n.commitencoding"));
    let now = OffsetDateTime::now_utc();

    // When amending, preserve original author unless explicitly overridden
    let amend_author = if args.amend
        && !args.reset_author
        && args.author.is_none()
        && args.reuse_message.is_none()
        && args.reedit_message.is_none()
        && args.date.is_none()
    {
        if let Some(head_oid) = head.oid() {
            let obj = repo.odb.read(head_oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            validate_amend_source_author(&commit.author)?;
            Some(commit.author)
        } else {
            None
        }
    } else {
        None
    };
    let author = if let Some(preserved) = amend_author {
        preserved
    } else {
        resolve_author(&args, &config, &repo, now)?
    };
    let committer = resolve_committer(&config, now)?;

    // Append Signed-off-by trailer if --signoff
    if args.signoff {
        let trailer = if let Some(angle_end) = committer.find('>') {
            format!("Signed-off-by: {}", &committer[..=angle_end])
        } else {
            format!("Signed-off-by: {committer}")
        };
        if !message.contains(&trailer) {
            let trimmed = message.trim_end();
            message = format!("{trimmed}\n\n{trailer}\n");
            // Also update raw_message if present
            if let Some(ref raw) = raw_message {
                let trimmed_raw = {
                    let mut end = raw.len();
                    while end > 0
                        && (raw[end - 1] == b'\n' || raw[end - 1] == b' ' || raw[end - 1] == b'\r')
                    {
                        end -= 1;
                    }
                    &raw[..end]
                };
                let mut new_raw = trimmed_raw.to_vec();
                new_raw.extend_from_slice(format!("\n\n{trailer}\n").as_bytes());
                raw_message = Some(new_raw);
            }
        }
    }

    // Run commit-msg hook with temp file containing the message
    {
        let msg_file = repo.git_dir.join("COMMIT_EDITMSG");
        // Write raw bytes when available so the hook sees the original encoding
        if let Some(ref raw) = raw_message {
            fs::write(&msg_file, raw)?;
        } else {
            fs::write(&msg_file, &message)?;
        }
        let msg_path_str = msg_file.to_string_lossy().to_string();
        match run_hook(&repo, "commit-msg", &[&msg_path_str], None) {
            HookResult::Failed(code) => {
                bail!("commit-msg hook exited with status {code}");
            }
            HookResult::Success => {
                // Re-read the message in case the hook modified it
                let new_raw = fs::read(&msg_file)?;
                match String::from_utf8(new_raw.clone()) {
                    Ok(s) => {
                        message = s;
                        raw_message = None;
                    }
                    Err(_) => {
                        message = String::from_utf8_lossy(&new_raw).to_string();
                        raw_message = Some(new_raw);
                    }
                }
            }
            _ => {}
        }
    }

    message = ensure_trailing_newline(&message);
    if let Some(ref mut raw) = raw_message {
        if !raw.ends_with(b"\n") {
            raw.push(b'\n');
        }
    }

    // Build commit object — set encoding header when i18n.commitEncoding is configured
    // and differs from UTF-8.
    let encoding = match &commit_encoding {
        Some(enc) if !enc.eq_ignore_ascii_case("utf-8") && !enc.eq_ignore_ascii_case("utf8") => {
            Some(enc.clone())
        }
        _ => None,
    };
    let commit_data = CommitData {
        tree: tree_oid,
        parents,
        author,
        committer,
        encoding,
        message,
        raw_message,
    };

    let commit_bytes = serialize_commit(&commit_data);
    let commit_oid = repo.odb.write(ObjectKind::Commit, &commit_bytes)?;

    // Update HEAD
    let old_oid = head
        .oid()
        .copied()
        .unwrap_or_else(|| ObjectId::from_bytes(&[0u8; 20]).unwrap());
    update_head(&repo.git_dir, &head, &commit_oid)?;

    // Write reflog entries
    {
        let msg = if head.is_unborn() {
            format!(
                "commit (initial): {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        } else if args.amend {
            format!(
                "commit (amend): {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        } else {
            format!(
                "commit: {}",
                commit_data.message.lines().next().unwrap_or("")
            )
        };
        // Write to HEAD reflog
        let _ = append_reflog(
            &repo.git_dir,
            "HEAD",
            &old_oid,
            &commit_oid,
            &commit_data.committer,
            &msg,
            false,
        );
        // Write to branch reflog if on a branch
        if let HeadState::Branch { refname, .. } = &head {
            let _ = append_reflog(
                &repo.git_dir,
                refname,
                &old_oid,
                &commit_oid,
                &commit_data.committer,
                &msg,
                false,
            );
        }
    }

    let _ = grit_lib::rerere::rerere_post_commit(&repo);
    cleanup_merge_state(&repo.git_dir);

    // Refresh the index file Git used for this commit (including `GIT_INDEX_FILE`).
    let mut index_refresh = match repo.load_index_at(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };
    repo.write_index_at(&index_path, &mut index_refresh)?;

    // Run post-commit hook (informational, don't abort on failure)
    let _ = run_hook(&repo, "post-commit", &[], None);

    // Run post-rewrite hook for --amend (unless --no-post-rewrite)
    if args.amend && !args.no_post_rewrite {
        if let Some(old_oid) = old_head_oid {
            let stdin_data = format!("{} {}\n", old_oid.to_hex(), commit_oid.to_hex());
            let _ = run_hook(
                &repo,
                "post-rewrite",
                &["amend"],
                Some(stdin_data.as_bytes()),
            );
        }
    }

    // Output summary
    if !args.quiet {
        let branch = match &head {
            HeadState::Branch { short_name, .. } => short_name.as_str(),
            HeadState::Detached { .. } => "HEAD detached",
            HeadState::Invalid => "unknown",
        };
        let short_oid = &commit_oid.to_hex()[..7];
        let first_line = commit_data.message.lines().next().unwrap_or("");
        if head.is_unborn() {
            println!("[{branch} (root-commit) {short_oid}] {first_line}");
        } else {
            println!("[{branch} {short_oid}] {first_line}");
        }

        // Print diff stat summary line
        let parent_tree = if commit_data.parents.is_empty() {
            None
        } else {
            let parent_obj = repo.odb.read(&commit_data.parents[0])?;
            let parent_commit = grit_lib::objects::parse_commit(&parent_obj.data)?;
            Some(parent_commit.tree)
        };
        if let Ok(diff_entries) =
            grit_lib::diff::diff_trees(&repo.odb, parent_tree.as_ref(), Some(&commit_data.tree), "")
        {
            let zero_oid = ObjectId::from_bytes(&[0u8; 20]).unwrap();
            let mut total_files = 0usize;
            let mut total_ins = 0usize;
            let mut total_del = 0usize;
            for entry in &diff_entries {
                total_files += 1;
                let old_content = if entry.old_oid == zero_oid {
                    String::new()
                } else {
                    repo.odb
                        .read(&entry.old_oid)
                        .map(|o| String::from_utf8_lossy(&o.data).into_owned())
                        .unwrap_or_default()
                };
                let new_content = if entry.new_oid == zero_oid {
                    String::new()
                } else {
                    repo.odb
                        .read(&entry.new_oid)
                        .map(|o| String::from_utf8_lossy(&o.data).into_owned())
                        .unwrap_or_default()
                };
                let (a, d) = grit_lib::diff::count_changes(&old_content, &new_content);
                total_ins += a;
                total_del += d;
            }
            if total_files > 0 {
                let mut summary = format!(
                    " {} file{} changed",
                    total_files,
                    if total_files == 1 { "" } else { "s" }
                );
                if total_ins > 0 {
                    summary.push_str(&format!(
                        ", {} insertion{}(+)",
                        total_ins,
                        if total_ins == 1 { "" } else { "s" }
                    ));
                }
                if total_del > 0 {
                    summary.push_str(&format!(
                        ", {} deletion{}(-)",
                        total_del,
                        if total_del == 1 { "" } else { "s" }
                    ));
                }
                println!("{summary}");
            }
        }
    }

    Ok(())
}

/// Print dry-run output (like `git commit --dry-run`).
fn print_dry_run(
    head: &HeadState,
    staged: &[DiffEntry],
    unstaged: &[DiffEntry],
    untracked: &[String],
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    match head {
        HeadState::Branch {
            short_name,
            oid: Some(_),
            ..
        } => {
            writeln!(out, "On branch {short_name}")?;
        }
        HeadState::Branch {
            short_name,
            oid: None,
            ..
        } => {
            writeln!(out, "On branch {short_name}")?;
            writeln!(out)?;
            writeln!(out, "No commits yet")?;
        }
        HeadState::Detached { oid } => {
            let short = &oid.to_hex()[..7];
            writeln!(out, "HEAD detached at {short}")?;
        }
        HeadState::Invalid => {
            writeln!(out, "Not currently on any branch.")?;
        }
    }

    if !staged.is_empty() {
        writeln!(out)?;
        writeln!(out, "Changes to be committed:")?;
        writeln!(out, "  (use \"git restore --staged <file>...\" to unstage)")?;
        for entry in staged {
            let label = status_label_staged(entry.status);
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
    }

    if !unstaged.is_empty() {
        writeln!(out)?;
        writeln!(out, "Changes not staged for commit:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to update what will be committed)"
        )?;
        for entry in unstaged {
            let label = status_label_unstaged(entry.status);
            writeln!(out, "\t{label}:   {}", entry.path())?;
        }
    }

    if !untracked.is_empty() {
        writeln!(out)?;
        writeln!(out, "Untracked files:")?;
        writeln!(
            out,
            "  (use \"git add <file>...\" to include in what will be committed)"
        )?;
        for path in untracked {
            writeln!(out, "\t{path}")?;
        }
    }

    Ok(())
}

fn status_label_staged(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Added => "new file",
        DiffStatus::Deleted => "deleted",
        DiffStatus::Modified => "modified",
        DiffStatus::Renamed => "renamed",
        DiffStatus::TypeChanged => "typechange",
        _ => "changed",
    }
}

fn status_label_unstaged(status: DiffStatus) -> &'static str {
    match status {
        DiffStatus::Deleted => "deleted",
        DiffStatus::Modified => "modified",
        DiffStatus::TypeChanged => "typechange",
        _ => "changed",
    }
}

/// Find untracked files in the working tree.
fn find_untracked_files(work_tree: &Path, index: &Index) -> Result<Vec<String>> {
    let tracked: BTreeSet<String> = index
        .entries
        .iter()
        .map(|ie| String::from_utf8_lossy(&ie.path).to_string())
        .collect();

    let mut untracked = Vec::new();
    walk_untracked(work_tree, work_tree, &tracked, &mut untracked)?;
    untracked.sort();
    Ok(untracked)
}

fn walk_untracked(
    dir: &Path,
    work_tree: &Path,
    tracked: &BTreeSet<String>,
    out: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());
    for entry in sorted {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let rel = path
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name);
        if path.is_dir() {
            let prefix = format!("{rel}/");
            let has_tracked = tracked.iter().any(|t| t.starts_with(&prefix));
            if has_tracked {
                walk_untracked(&path, work_tree, tracked, out)?;
            } else {
                out.push(format!("{rel}/"));
            }
        } else if !tracked.contains(&rel) {
            out.push(rel);
        }
    }
    Ok(())
}

/// Stage specific files given as pathspec arguments to `commit`.
///
/// Returns the set of repository-relative paths that were staged (or removed) for this commit.
fn stage_pathspec_files(
    repo: &Repository,
    work_tree: &Path,
    pathspecs: &[String],
) -> Result<HashSet<Vec<u8>>> {
    use std::os::unix::fs::MetadataExt;

    let index_path = resolved_index_path(repo);
    let mut index = match repo.load_index_at(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| work_tree.to_path_buf());
    let prefix = crate::pathspec::pathdiff(&cwd, work_tree);

    let mut matched_paths = HashSet::new();

    for spec in pathspecs {
        let resolved = crate::pathspec::resolve_pathspec(spec, work_tree, prefix.as_deref());
        if !crate::pathspec::has_glob_chars(&resolved) {
            let abs_path = work_tree.join(&resolved);
            if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                let data = if meta.file_type().is_symlink() {
                    let target = fs::read_link(&abs_path)?;
                    target.to_string_lossy().into_owned().into_bytes()
                } else {
                    fs::read(&abs_path)?
                };
                let oid = repo.odb.write(ObjectKind::Blob, &data)?;
                let mode = grit_lib::index::normalize_mode(meta.mode());
                let raw_path = resolved.as_bytes().to_vec();
                let entry = grit_lib::index::entry_from_stat(&abs_path, &raw_path, oid, mode)?;
                index.add_or_replace(entry);
                matched_paths.insert(raw_path);
            } else {
                index.remove(resolved.as_bytes());
                matched_paths.insert(resolved.as_bytes().to_vec());
            }
            continue;
        }

        let (dir_prefix, pattern) = if let Some(slash_pos) = resolved.rfind('/') {
            (&resolved[..slash_pos], &resolved[slash_pos + 1..])
        } else {
            ("", resolved.as_str())
        };

        let search_dir = if dir_prefix.is_empty() {
            work_tree.to_path_buf()
        } else {
            work_tree.join(dir_prefix)
        };

        let mut spec_matched = false;
        let mut matched_rels: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let name_str = entry.file_name().to_string_lossy().to_string();
                if name_str == ".git" {
                    continue;
                }
                if !grit_lib::wildmatch::wildmatch(pattern.as_bytes(), name_str.as_bytes(), 0) {
                    continue;
                }
                let rel = if dir_prefix.is_empty() {
                    name_str.clone()
                } else {
                    format!("{dir_prefix}/{name_str}")
                };
                matched_rels.push(rel);
            }
        }
        if pattern.contains('[') && fs::symlink_metadata(search_dir.join(pattern)).is_ok() {
            let rel = if dir_prefix.is_empty() {
                pattern.to_string()
            } else {
                format!("{dir_prefix}/{pattern}")
            };
            if !matched_rels.contains(&rel) {
                matched_rels.push(rel);
            }
        }

        for rel in matched_rels {
            let abs_path = work_tree.join(&rel);
            if let Ok(meta) = fs::symlink_metadata(&abs_path) {
                let data = if meta.file_type().is_symlink() {
                    let target = fs::read_link(&abs_path)?;
                    target.to_string_lossy().into_owned().into_bytes()
                } else {
                    fs::read(&abs_path)?
                };
                let oid = repo.odb.write(ObjectKind::Blob, &data)?;
                let mode = grit_lib::index::normalize_mode(meta.mode());
                let raw_path = rel.as_bytes().to_vec();
                let entry = grit_lib::index::entry_from_stat(&abs_path, &raw_path, oid, mode)?;
                index.add_or_replace(entry);
                spec_matched = true;
                matched_paths.insert(raw_path);
            }
        }

        if !spec_matched {
            bail!("pathspec '{spec}' did not match any file(s) known to git");
        }
    }

    repo.write_index_at(&index_path, &mut index)?;
    Ok(matched_paths)
}

/// Auto-stage tracked files (for `commit -a`).
fn auto_stage_tracked(repo: &Repository, work_tree: &Path) -> Result<()> {
    let index_path = resolved_index_path(repo);
    let mut index = match repo.load_index_at(&index_path) {
        Ok(idx) => idx,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let tracked: Vec<(Vec<u8>, String, u32)> = index
        .entries
        .iter()
        .map(|ie| {
            let path_str = String::from_utf8_lossy(&ie.path).to_string();
            (ie.path.clone(), path_str, ie.mode)
        })
        .collect();

    let mut changed = false;
    for (raw_path, path_str, idx_mode) in &tracked {
        let abs_path = work_tree.join(path_str);
        // Use `symlink_metadata`, not `exists()`: `Path::exists` follows symlinks, so
        // dangling symlinks look "missing" and would be dropped from the index (t1006).
        if fs::symlink_metadata(&abs_path).is_ok() {
            // Gitlink (submodule) entries: read the embedded repo's HEAD to
            // get the current commit OID instead of trying to read the
            // directory as a file.
            if *idx_mode == 0o160000 {
                let head_path = abs_path.join(".git/HEAD");
                if let Ok(head_content) = fs::read_to_string(&head_path) {
                    let head_trimmed = head_content.trim();
                    let oid_hex = if let Some(r) = head_trimmed.strip_prefix("ref: ") {
                        let ref_path = abs_path.join(".git").join(r);
                        match fs::read_to_string(&ref_path) {
                            Ok(s) => s.trim().to_string(),
                            Err(_) => continue,
                        }
                    } else {
                        head_trimmed.to_string()
                    };
                    if let Ok(oid) = oid_hex.parse::<ObjectId>() {
                        use std::os::unix::fs::MetadataExt;
                        let meta = fs::symlink_metadata(&abs_path)?;
                        let entry = grit_lib::index::IndexEntry {
                            ctime_sec: meta.ctime() as u32,
                            ctime_nsec: meta.ctime_nsec() as u32,
                            mtime_sec: meta.mtime() as u32,
                            mtime_nsec: meta.mtime_nsec() as u32,
                            dev: meta.dev() as u32,
                            ino: meta.ino() as u32,
                            mode: 0o160000,
                            uid: meta.uid(),
                            gid: meta.gid(),
                            size: 0,
                            oid,
                            flags: path_str.len().min(0xFFF) as u16,
                            flags_extended: None,
                            path: raw_path.clone(),
                        };
                        index.add_or_replace(entry);
                        changed = true;
                    }
                }
                continue;
            }
            use std::os::unix::fs::MetadataExt;
            let meta = fs::symlink_metadata(&abs_path)?;
            let data = if meta.file_type().is_symlink() {
                let target = fs::read_link(&abs_path)?;
                target.to_string_lossy().into_owned().into_bytes()
            } else {
                fs::read(&abs_path)?
            };
            let oid = repo.odb.write(ObjectKind::Blob, &data)?;
            if index
                .entries
                .iter()
                .find(|e| e.path == *raw_path)
                .is_some_and(|e| e.oid == oid)
            {
                continue;
            }
            let mode = grit_lib::index::normalize_mode(meta.mode());
            let entry = grit_lib::index::entry_from_stat(&abs_path, raw_path, oid, mode)?;
            index.add_or_replace(entry);
            changed = true;
        } else {
            index.remove(raw_path);
            changed = true;
        }
    }

    if changed {
        repo.write_index_at(&index_path, &mut index)?;
    }

    Ok(())
}

/// Result of building a commit message — may be UTF-8 or raw bytes.
struct MessageResult {
    /// UTF-8 message (always set; lossy if raw_bytes is Some).
    message: String,
    /// Raw bytes when the message is not valid UTF-8.
    raw_bytes: Option<Vec<u8>>,
}

fn resolved_index_path(repo: &Repository) -> PathBuf {
    if let Ok(raw) = std::env::var("GIT_INDEX_FILE") {
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            p
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(p)
        } else {
            p
        }
    } else {
        repo.index_path()
    }
}

fn parse_fixup_argument(raw: &str) -> Result<FixupParsed> {
    let (prefix, rest) = match raw.split_once(':') {
        Some((a, b)) if !a.is_empty() && a.chars().all(|c| c.is_ascii_alphabetic()) => (a, b),
        _ => {
            return Ok(FixupParsed {
                mode: FixupMode::Fixup,
                commit_ref: raw.to_string(),
            });
        }
    };
    match prefix {
        "amend" => Ok(FixupParsed {
            mode: FixupMode::AmendStyle { is_reword: false },
            commit_ref: rest.to_string(),
        }),
        "reword" => Ok(FixupParsed {
            mode: FixupMode::AmendStyle { is_reword: true },
            commit_ref: rest.to_string(),
        }),
        _ => bail!("unknown option: --fixup={prefix}:{rest}"),
    }
}

fn commit_rename_settings(config: &ConfigSet) -> (Option<u32>, bool) {
    match config.get("diff.renames") {
        Some(val) => {
            let lowered = val.to_lowercase();
            match lowered.as_str() {
                "false" | "no" | "off" | "0" => (None, false),
                "true" | "yes" | "on" | "1" | "" => (Some(50), false),
                "copies" | "copy" => (Some(50), true),
                _ => (None, false),
            }
        }
        None => (Some(50), false),
    }
}

fn commit_uses_editor(args: &Args, fixup: Option<&FixupParsed>) -> bool {
    if args.reuse_message.is_some() && args.reedit_message.is_none() {
        return false;
    }
    if !args.message.is_empty() || args.file.is_some() {
        return false;
    }
    if let Some(f) = fixup {
        match f.mode {
            // Plain `--fixup` uses a generated message unless `--edit` forces the editor.
            FixupMode::Fixup => return args.edit,
            FixupMode::AmendStyle { .. } => return true,
        }
    }
    true
}

fn parse_optional_path_spec(spec: &str) -> (bool, &str) {
    const OPT: &str = ":(optional)";
    if let Some(rest) = spec.strip_prefix(OPT) {
        (true, rest)
    } else {
        (false, spec)
    }
}

fn resolve_commit_template_path(args: &Args, config: &ConfigSet) -> Result<Option<PathBuf>> {
    let cli = args.template.as_deref();
    let cfg_owned = config.get("commit.template");
    let cfg = cfg_owned.as_deref();
    let chosen = cli.or(cfg);
    let Some(raw) = chosen else {
        return Ok(None);
    };
    let (optional, path_str) = parse_optional_path_spec(raw.trim());
    let path = Path::new(path_str);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    if abs.is_file() {
        return Ok(Some(abs));
    }
    if optional {
        return Ok(None);
    }
    bail!("fatal: could not read '{}'", abs.display());
}

fn first_line(message: &str) -> &str {
    message.lines().next().unwrap_or("").trim_end()
}

fn format_fixup_subject(repo: &Repository, prefix: &str, commit_ref: &str) -> Result<String> {
    let oid = resolve_revision(repo, commit_ref)?;
    let obj = repo.odb.read(&oid)?;
    let commit = grit_lib::objects::parse_commit(&obj.data)?;
    let subj = first_line(&commit.message);
    Ok(format!("{prefix}! {subj}\n\n"))
}

fn message_body_after_subject(full: &str) -> &str {
    if let Some(pos) = full.find("\n\n") {
        &full[pos + 2..]
    } else {
        ""
    }
}

fn skip_blank_lines(mut s: &str) -> &str {
    while let Some(rest) = s.strip_prefix('\n') {
        s = rest;
    }
    s
}

fn commit_body_for_amend_fixup(repo: &Repository, target_oid: &ObjectId) -> Result<String> {
    let obj = repo.odb.read(target_oid)?;
    let commit = grit_lib::objects::parse_commit(&obj.data)?;
    let subj = first_line(&commit.message);
    // Match `prepare_amend_commit` in Git: if the target subject already begins with
    // `amend!`, format with `%b` only (drop the duplicated subject line from the body).
    let body = if subj.trim_start().starts_with("amend!") {
        message_body_after_subject(&commit.message)
    } else {
        commit.message.as_str()
    };
    Ok(skip_blank_lines(body).to_string())
}

fn message_after_first_line(message: &str) -> &str {
    message.find('\n').map(|i| &message[i + 1..]).unwrap_or("")
}

/// Git inserts a blank line between the autosquash subject and editor-appended body when the
/// template starts with `subject\n\n` (even if cleanup removed the second newline visually).
fn normalize_autosquash_editor_message(
    args: &Args,
    fixup: Option<&FixupParsed>,
    used_editor: bool,
    message: &str,
) -> String {
    if !used_editor
        || args.file.is_some()
        || args.reuse_message.is_some()
        || args.reedit_message.is_some()
    {
        return message.to_string();
    }
    if args.squash.is_none() {
        return message.to_string();
    }
    if fixup.is_some() {
        return message.to_string();
    }
    let Some(first_nl) = message.find('\n') else {
        return message.to_string();
    };
    let first_line = &message[..first_nl];
    let rest = &message[first_nl + 1..];
    let rest_trim = rest.trim_start_matches(['\n', '\r']);
    if rest_trim.is_empty() {
        return message.to_string();
    }
    if rest.starts_with("\n\n") || rest.starts_with("\r\n\r\n") {
        return message.to_string();
    }
    format!("{first_line}\n\n{rest_trim}")
}

fn build_squash_prefix(
    repo: &Repository,
    squash_ref: &str,
    reuse_rev: Option<&str>,
) -> Result<String> {
    if reuse_rev == Some(squash_ref) {
        return Ok("squash! ".to_string());
    }
    format_fixup_subject(repo, "squash", squash_ref)
}

fn read_message_file_raw(file_path: &str) -> Result<Vec<u8>> {
    if file_path == "-" {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        fs::read(file_path).with_context(|| format!("could not read log file '{file_path}'"))
    }
}

fn raw_to_message_result(raw: Vec<u8>) -> Result<MessageResult> {
    match String::from_utf8(raw.clone()) {
        Ok(s) => Ok(MessageResult {
            message: ensure_trailing_newline(&s),
            raw_bytes: None,
        }),
        Err(_) => {
            let lossy = String::from_utf8_lossy(&raw).to_string();
            let mut raw_nl = raw;
            if !raw_nl.ends_with(b"\n") {
                raw_nl.push(b'\n');
            }
            Ok(MessageResult {
                message: ensure_trailing_newline(&lossy),
                raw_bytes: Some(raw_nl),
            })
        }
    }
}

fn build_initial_commit_buffer(
    args: &Args,
    repo: &Repository,
    fixup: Option<&FixupParsed>,
    template_path: Option<&Path>,
) -> Result<String> {
    let mut buf = String::new();

    if fixup.is_none() && !args.message.is_empty() {
        buf.push_str(&args.message.join("\n\n"));
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        return Ok(buf);
    }

    if let Some(fp) = fixup {
        match &fp.mode {
            FixupMode::Fixup => {
                buf.push_str(&format_fixup_subject(repo, "fixup", &fp.commit_ref)?);
                if !args.message.is_empty() {
                    buf.push_str(&args.message.join("\n\n"));
                }
                if !buf.ends_with('\n') {
                    buf.push('\n');
                }
                return Ok(buf);
            }
            FixupMode::AmendStyle { .. } => {
                buf.push_str(&format_fixup_subject(repo, "amend", &fp.commit_ref)?);
                let oid = resolve_revision(repo, &fp.commit_ref)?;
                buf.push_str(&commit_body_for_amend_fixup(repo, &oid)?);
                if !buf.ends_with('\n') {
                    buf.push('\n');
                }
                return Ok(buf);
            }
        }
    }

    if let Some(ref file_path) = args.file {
        let raw = read_message_file_raw(file_path)?;
        let text = String::from_utf8_lossy(&raw);
        buf.push_str(text.as_ref());
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        return Ok(buf);
    }

    let reuse_rev = args.reuse_message.as_ref().or(args.reedit_message.as_ref());
    if let Some(rev) = reuse_rev {
        let oid = resolve_revision(repo, rev)?;
        let obj = repo.odb.read(&oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        let body = skip_blank_lines(message_body_after_subject(&commit.message));
        buf.push_str(body);
        if !buf.is_empty() && !buf.ends_with('\n') {
            buf.push('\n');
        }
        return Ok(buf);
    }

    if let Some(msg) = grit_lib::state::read_merge_msg(&repo.git_dir)? {
        buf.push_str(&msg);
        return Ok(buf);
    }

    let squash_msg_path = repo.git_dir.join("SQUASH_MSG");
    if let Ok(msg) = fs::read_to_string(&squash_msg_path) {
        if !msg.is_empty() {
            buf.push_str(&msg);
            return Ok(buf);
        }
    }

    if let Some(tpl) = template_path {
        buf.push_str(
            &fs::read_to_string(tpl)
                .with_context(|| format!("fatal: could not read '{}'", tpl.display()))?,
        );
        return Ok(buf);
    }

    if args.amend {
        let head = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head.oid() {
            let obj = repo.odb.read(oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            buf.push_str(&commit.message);
            return Ok(buf);
        }
    }

    Ok(buf)
}

fn is_effective_editor_value(raw: &str) -> bool {
    let t = raw.trim();
    !t.is_empty() && t != ":"
}

fn resolve_commit_editor(repo: &Repository) -> String {
    let visual_present = std::env::var("VISUAL").is_ok();
    let editor_present = std::env::var("EDITOR").is_ok();

    if let Ok(e) = std::env::var("GIT_EDITOR") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if let Ok(config) = ConfigSet::load(Some(&repo.git_dir), true) {
        if let Some(e) = config.get("core.editor") {
            if is_effective_editor_value(&e) {
                return e;
            }
        }
    }
    // Git order: VISUAL then EDITOR. Skip `:` / empty `VISUAL` (test harness sets `VISUAL=:`).
    if let Ok(e) = std::env::var("VISUAL") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    if let Ok(e) = std::env::var("EDITOR") {
        if is_effective_editor_value(&e) {
            return e;
        }
    }
    // Harness sets `EDITOR=:` / `VISUAL=:` as non-interactive placeholders; never launch `vi`
    // in that case (would hang). Fall back to `true` like a no-op editor.
    if visual_present || editor_present {
        "true".to_owned()
    } else {
        "vi".to_owned()
    }
}

fn launch_commit_editor(repo: &Repository, path: &Path) -> Result<()> {
    let editor = resolve_commit_editor(repo);
    // Match Git: the editor command is run under `sh -c` with the path as `$1` (not `$@`),
    // so `test_set_editor` patterns like `EDITOR='"$FAKE_EDITOR"'` expand and receive the file.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;
    if !status.success() {
        bail!("editor exited with non-zero status");
    }
    Ok(())
}

/// Post-editor cleanup matching Git `strbuf_stripspace` with `comment_prefix = "#"` (default
/// `cleanup=strip`): skip `#` lines, trim trailing whitespace per line, collapse runs of empty
/// lines to a single blank between paragraphs, trim leading/trailing blank lines.
fn cleanup_edited_commit_message(message: &str) -> String {
    fn line_cleanup(line: &str) -> usize {
        let mut len = line.len();
        while len > 0 {
            let c = line.as_bytes()[len - 1];
            if !c.is_ascii_whitespace() {
                break;
            }
            len -= 1;
        }
        len
    }

    let mut out = String::new();
    let mut empties = 0usize;
    let mut i = 0usize;
    while i < message.len() {
        let rest = &message[i..];
        let (line_with_nl, advance) = if let Some(pos) = rest.find('\n') {
            (&rest[..=pos], pos + 1)
        } else {
            (rest, rest.len())
        };
        i += advance;

        if line_with_nl.starts_with('#') {
            continue;
        }
        let content_len = line_cleanup(line_with_nl);
        if content_len > 0 {
            if empties > 0 && !out.is_empty() {
                out.push('\n');
            }
            empties = 0;
            out.push_str(&line_with_nl[..content_len]);
            out.push('\n');
        } else {
            empties += 1;
        }
    }
    out
}

fn git_vertical_stripspace(s: &str) -> String {
    let trimmed_start = s.trim_start_matches(['\n', '\r', ' ', '\t']);
    trimmed_start
        .trim_end_matches(['\n', '\r', ' ', '\t'])
        .to_string()
}

fn rest_is_empty_signedoff_only(s: &str, start: usize) -> bool {
    const SOB: &str = "Signed-off-by:";
    let rest = s.get(start..).unwrap_or("");
    for line in rest.split_inclusive('\n') {
        let line_no_nl = line.strip_suffix('\n').unwrap_or(line);
        let t = line_no_nl.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with(SOB) {
            continue;
        }
        return false;
    }
    true
}

fn template_untouched(message: &str, template_path: &Path) -> bool {
    let Ok(tmpl_raw) = fs::read_to_string(template_path) else {
        return false;
    };
    // Git runs `cleanup_message` before `template_untouched`, so `#` lines are stripped from
    // both the editor buffer and the template file content for this comparison.
    let tmpl = cleanup_edited_commit_message(&tmpl_raw);
    let msg = cleanup_edited_commit_message(message);
    let after_prefix = msg.strip_prefix(&tmpl).unwrap_or(msg.as_str());
    rest_is_empty_signedoff_only(msg.as_str(), msg.len().saturating_sub(after_prefix.len()))
}

fn branch_display_name(head: &HeadState) -> String {
    match head {
        HeadState::Branch { short_name, .. } => short_name.clone(),
        HeadState::Detached { .. } => "HEAD detached".to_string(),
        HeadState::Invalid => "unknown".to_string(),
    }
}

fn git_binary_for_status() -> PathBuf {
    if let Ok(exec) = std::env::var("GIT_EXEC_PATH") {
        let candidate = Path::new(&exec).join("git");
        if candidate.is_file() {
            return candidate;
        }
    }
    // Tests prepend a `git` wrapper that runs grit; invoking `git diff` from commit
    // would recurse. Prefer the real host binary when available.
    for path in ["/usr/bin/git", "/bin/git"] {
        let p = PathBuf::from(path);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("git")
}

fn commit_template_status_append(
    args: &Args,
    repo: &Repository,
    head: &HeadState,
    config: &ConfigSet,
    buf: &mut String,
) -> Result<()> {
    buf.push('\n');
    if args.allow_empty_message {
        buf.push_str(
            "# Please enter the commit message for your changes. Lines starting\n\
             # with '#' will be ignored.\n",
        );
    } else {
        buf.push_str(
            "# Please enter the commit message for your changes. Lines starting\n\
             # with '#' will be ignored, and an empty message aborts the commit.\n",
        );
    }
    if args.allow_empty_message {
        buf.push_str("#\n");
    }
    let author = resolve_author(args, config, repo, OffsetDateTime::now_utc())?;
    buf.push_str("# Author:    ");
    let author_display = author
        .split_once('>')
        .map(|(a, _)| format!("{}>", a.trim()))
        .unwrap_or_else(|| author.clone());
    buf.push_str(&author_display);
    buf.push('\n');
    buf.push_str("#\n");
    buf.push_str("# On branch ");
    buf.push_str(&branch_display_name(head));
    buf.push('\n');
    buf.push_str("# Changes to be committed:\n");

    if let Some(wt) = repo.work_tree.as_deref() {
        let index_file = resolved_index_path(repo);
        let output = Command::new(git_binary_for_status())
            .current_dir(wt)
            .env("GIT_DIR", &repo.git_dir)
            .env("GIT_INDEX_FILE", &index_file)
            .args(["diff", "--cached", "--name-status"])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    let line = line.trim_end();
                    if line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.is_empty() {
                        continue;
                    }
                    let status = parts[0];
                    let (label, display_path) =
                        if status.starts_with('R') || status.starts_with('C') {
                            if parts.len() >= 3 {
                                let lbl = if status.starts_with('R') {
                                    "renamed"
                                } else {
                                    "copied"
                                };
                                (lbl, format!("{} -> {}", parts[1], parts[2]))
                            } else {
                                continue;
                            }
                        } else {
                            let lbl = match status.chars().next() {
                                Some('A') => "new file",
                                Some('D') => "deleted",
                                Some('M') => "modified",
                                Some('T') => "typechange",
                                _ => "changed",
                            };
                            let p = parts.get(1).copied().unwrap_or("");
                            (lbl, p.to_string())
                        };
                    buf.push_str(&format!("#\t{label}:   {display_path}\n"));
                }
                buf.push_str("#\n");
                buf.push_str("# Untracked files not listed\n");
                return Ok(());
            }
        }
    }

    let index = match repo.load_index_at(&resolved_index_path(repo)) {
        Ok(i) => i,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Index::new(),
        Err(e) => return Err(e.into()),
    };
    let head_tree = match head.oid() {
        Some(oid) => {
            let obj = repo.odb.read(oid)?;
            let c = grit_lib::objects::parse_commit(&obj.data)?;
            Some(c.tree)
        }
        None => None,
    };
    let staged = diff_index_to_tree(&repo.odb, &index, head_tree.as_ref())?;
    for e in &staged {
        let label = status_label_staged(e.status);
        buf.push_str(&format!("#\t{label}:   {}\n", e.display_path()));
    }
    buf.push_str("#\n");
    buf.push_str("# Untracked files not listed\n");
    Ok(())
}

fn prepare_commit_message(
    args: &Args,
    repo: &Repository,
    config: &ConfigSet,
    fixup: Option<&FixupParsed>,
    template_path: Option<&Path>,
    use_editor: bool,
    head: &HeadState,
) -> Result<MessageResult> {
    if let Some(sq) = args.squash.as_deref() {
        let reuse = args
            .reuse_message
            .as_deref()
            .or(args.reedit_message.as_deref());
        let prefix = build_squash_prefix(repo, sq, reuse)?;
        let mut body = String::new();
        if !args.message.is_empty() {
            body.push_str(&args.message.join("\n\n"));
        } else if let Some(ref fp) = args.file {
            let raw = read_message_file_raw(fp)?;
            body.push_str(&String::from_utf8_lossy(&raw));
        } else if let Some(rev) = reuse {
            let oid = resolve_revision(repo, rev)?;
            let obj = repo.odb.read(&oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            if args.reedit_message.is_some() {
                let edit_path = repo.git_dir.join("COMMIT_EDITMSG");
                let mut file_body = prefix.clone();
                file_body.push_str(&commit.message);
                if !args.no_status {
                    commit_template_status_append(args, repo, head, config, &mut file_body)?;
                }
                fs::write(&edit_path, &file_body)?;
                launch_commit_editor(repo, &edit_path)?;
                let edited = fs::read_to_string(&edit_path)?;
                let cleaned = cleanup_edited_commit_message(&edited);
                return Ok(MessageResult {
                    message: ensure_trailing_newline(&cleaned),
                    raw_bytes: None,
                });
            }
            if rev == sq {
                let subj = first_line(&commit.message);
                body.push_str(subj);
            } else {
                // `-C`: reuse the full commit log (including its subject) after the squash prefix.
                body.push_str(&commit.message);
            }
        } else if use_editor {
            let edit_path = repo.git_dir.join("COMMIT_EDITMSG");
            let mut file_body = prefix.clone();
            if file_body.trim().is_empty() {
                file_body.push('\n');
            }
            if !args.no_status {
                commit_template_status_append(args, repo, head, config, &mut file_body)?;
            }
            fs::write(&edit_path, &file_body)?;
            launch_commit_editor(repo, &edit_path)?;
            let edited = fs::read_to_string(&edit_path)?;
            let cleaned = cleanup_edited_commit_message(&edited);
            return Ok(MessageResult {
                message: ensure_trailing_newline(&cleaned),
                raw_bytes: None,
            });
        }
        let combined = format!("{prefix}{body}");
        return Ok(MessageResult {
            message: ensure_trailing_newline(&combined),
            raw_bytes: None,
        });
    }

    if !args.message.is_empty() && fixup.map(|f| matches!(f.mode, FixupMode::Fixup)) != Some(true) {
        let msg = args.message.join("\n\n");
        return Ok(MessageResult {
            message: ensure_trailing_newline(&msg),
            raw_bytes: None,
        });
    }

    if let Some(ref file_path) = args.file {
        return raw_to_message_result(read_message_file_raw(file_path)?);
    }

    let reuse_rev = args.reuse_message.as_ref().or(args.reedit_message.as_ref());
    if let Some(rev) = reuse_rev {
        let oid = resolve_revision(repo, rev)?;
        let obj = repo.odb.read(&oid)?;
        let commit = grit_lib::objects::parse_commit(&obj.data)?;
        if args.reedit_message.is_some() {
            let edit_path = repo.git_dir.join("COMMIT_EDITMSG");
            let mut file_body = build_initial_commit_buffer(args, repo, fixup, template_path)?;
            if !args.no_status {
                commit_template_status_append(args, repo, head, config, &mut file_body)?;
            }
            fs::write(&edit_path, &file_body)?;
            launch_commit_editor(repo, &edit_path)?;
            let edited = fs::read_to_string(&edit_path)?;
            let cleaned = cleanup_edited_commit_message(&edited);
            return Ok(MessageResult {
                message: ensure_trailing_newline(&cleaned),
                raw_bytes: None,
            });
        }
        return Ok(MessageResult {
            message: commit.message,
            raw_bytes: None,
        });
    }

    let initial = build_initial_commit_buffer(args, repo, fixup, template_path)?;

    if args.allow_empty_message
        && initial.trim().is_empty()
        && template_path.is_none()
        && fixup.is_none()
        && args.squash.is_none()
        && !use_editor
    {
        return Ok(MessageResult {
            message: String::new(),
            raw_bytes: None,
        });
    }

    if !use_editor && fixup.is_some() {
        return Ok(MessageResult {
            message: ensure_trailing_newline(&initial),
            raw_bytes: None,
        });
    }

    if use_editor {
        let edit_path = repo.git_dir.join("COMMIT_EDITMSG");
        let mut file_body = initial;
        if !args.no_status {
            commit_template_status_append(args, repo, head, config, &mut file_body)?;
        }
        fs::write(&edit_path, &file_body)?;
        launch_commit_editor(repo, &edit_path)?;
        let edited = fs::read_to_string(&edit_path)?;
        let cleaned = cleanup_edited_commit_message(&edited);
        return Ok(MessageResult {
            message: ensure_trailing_newline(&cleaned),
            raw_bytes: None,
        });
    }

    if let Some(msg) = grit_lib::state::read_merge_msg(&repo.git_dir)? {
        return Ok(MessageResult {
            message: ensure_trailing_newline(&msg),
            raw_bytes: None,
        });
    }

    let squash_msg_path = repo.git_dir.join("SQUASH_MSG");
    if let Ok(msg) = fs::read_to_string(&squash_msg_path) {
        if !msg.is_empty() {
            return Ok(MessageResult {
                message: ensure_trailing_newline(&msg),
                raw_bytes: None,
            });
        }
    }

    if let Some(tpl) = template_path {
        let content = fs::read_to_string(tpl)
            .with_context(|| format!("fatal: could not read '{}'", tpl.display()))?;
        return Ok(MessageResult {
            message: ensure_trailing_newline(&content),
            raw_bytes: None,
        });
    }

    if args.amend {
        let head_st = resolve_head(&repo.git_dir)?;
        if let Some(oid) = head_st.oid() {
            let obj = repo.odb.read(oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            return Ok(MessageResult {
                message: commit.message,
                raw_bytes: None,
            });
        }
    }

    if args.allow_empty_message {
        return Ok(MessageResult {
            message: String::new(),
            raw_bytes: None,
        });
    }

    bail!("no commit message provided (use -m or -F)");
}

/// Parse `git commit --author="Name <email>"` parameter into name and email.
fn parse_force_author_parameter(author: &str) -> Result<(String, String)> {
    let Some(lt) = author.find('<') else {
        bail!("malformed --author parameter");
    };
    let Some(gt) = author.rfind('>') else {
        bail!("malformed --author parameter");
    };
    if gt <= lt {
        bail!("malformed --author parameter");
    }
    let name = author[..lt].trim_end();
    let email = author[lt + 1..gt].trim();
    if name.is_empty() {
        bail!("empty ident name (for <author>) not allowed");
    }
    if email.is_empty() {
        bail!("malformed --author parameter");
    }
    if lt > 0 && author.as_bytes()[lt - 1] != b' ' {
        bail!("malformed --author parameter");
    }
    Ok((name.to_string(), email.to_string()))
}

/// Split a stored author line (`name <email> <epoch> <tz>`) into name, email, and optional date tail.
fn split_stored_author_line(author: &str) -> Result<(String, String, Option<String>)> {
    let Some(lt) = author.find('<') else {
        bail!("malformed author line");
    };
    let Some(gt) = author.rfind('>') else {
        bail!("malformed author line");
    };
    if gt <= lt {
        bail!("malformed author line");
    }
    let name = author[..lt].trim_end();
    let email = author[lt + 1..gt].trim();
    let after_gt = author[gt + 1..].trim_start();
    let date_tail = if after_gt.is_empty() {
        None
    } else {
        Some(after_gt.to_string())
    };
    Ok((name.to_string(), email.to_string(), date_tail))
}

/// Reject empty/malformed author identity when amending (matches Git's strictness for t7509).
fn validate_amend_source_author(author: &str) -> Result<()> {
    let (name, email, date_tail) = split_stored_author_line(author)
        .map_err(|_| anyhow::anyhow!("commit has malformed author line"))?;
    if name.is_empty() {
        bail!("empty ident name (for <author>) not allowed");
    }
    validate_ident_name(&name, "author")?;
    if email.is_empty() {
        bail!("empty ident name (for <author>) not allowed");
    }
    if date_tail.is_none() || date_tail.as_ref().is_some_and(|s| s.is_empty()) {
        bail!("empty ident name (for <author>) not allowed");
    }
    Ok(())
}

fn read_cherry_pick_head_author(repo: &Repository) -> Result<Option<String>> {
    let path = repo.git_dir.join("CHERRY_PICK_HEAD");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).context("read CHERRY_PICK_HEAD")?;
    let hex = content.trim();
    if hex.is_empty() {
        return Ok(None);
    }
    let oid: ObjectId = hex
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid CHERRY_PICK_HEAD"))?;
    let obj = repo.odb.read(&oid)?;
    let commit = grit_lib::objects::parse_commit(&obj.data)?;
    Ok(Some(commit.author))
}

/// Check if an ident name is valid (not empty and not all special characters).
fn validate_ident_name(name: &str, kind: &str) -> Result<()> {
    let cleaned: String = name
        .chars()
        .filter(|&c| {
            c != '.'
                && c != ','
                && c != ';'
                && c != '<'
                && c != '>'
                && c != '\''
                && c != '"'
                && c != ' '
        })
        .collect();
    if cleaned.is_empty() {
        if name.is_empty() {
            bail!("empty ident name (for <{}>) not allowed", kind);
        } else {
            bail!("invalid ident name: '{}'", name);
        }
    }
    Ok(())
}

fn resolve_author(
    args: &Args,
    config: &ConfigSet,
    repo: &Repository,
    now: OffsetDateTime,
) -> Result<String> {
    if let Some(ref author) = args.author {
        let (name, email) = parse_force_author_parameter(author)?;
        validate_ident_name(&name, "author")?;
        let date_str = args
            .date
            .as_deref()
            .map(String::from)
            .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok());
        let timestamp = match date_str {
            Some(d) => parse_date_to_git_timestamp(&d).unwrap_or(d),
            None => format_git_timestamp(now),
        };
        return Ok(format!("{name} <{email}> {timestamp}"));
    }

    let reuse_rev = args.reuse_message.as_ref().or(args.reedit_message.as_ref());
    if let Some(rev) = reuse_rev {
        if !args.reset_author {
            let oid = resolve_revision(repo, rev)?;
            let obj = repo.odb.read(&oid)?;
            let commit = grit_lib::objects::parse_commit(&obj.data)?;
            if let Some(ref d) = args.date {
                let (name, email, _) = split_stored_author_line(&commit.author)?;
                validate_ident_name(&name, "author")?;
                let timestamp = parse_date_to_git_timestamp(d).unwrap_or_else(|| d.to_string());
                return Ok(format!("{name} <{email}> {timestamp}"));
            }
            return Ok(commit.author);
        }
    }

    if !args.reset_author {
        if let Some(cp_author) = read_cherry_pick_head_author(repo)? {
            if let Some(ref d) = args.date {
                let (name, email, _) = split_stored_author_line(&cp_author)?;
                validate_ident_name(&name, "author")?;
                let timestamp = parse_date_to_git_timestamp(d).unwrap_or_else(|| d.to_string());
                return Ok(format!("{name} <{email}> {timestamp}"));
            }
            return Ok(cp_author);
        }
    }

    let name = resolve_name(config, IdentRole::Author)?;
    validate_ident_name(&name, "author")?;

    let email = resolve_email(config, IdentRole::Author)?;

    let date_str = args
        .date
        .as_deref()
        .map(String::from)
        .or_else(|| std::env::var("GIT_AUTHOR_DATE").ok());

    let timestamp = match date_str {
        Some(d) => parse_date_to_git_timestamp(&d).unwrap_or(d),
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Resolve the committer identity from env and config.
fn resolve_committer(config: &ConfigSet, now: OffsetDateTime) -> Result<String> {
    let name = resolve_name(config, IdentRole::Committer)?;
    validate_ident_name(&name, "committer")?;

    let email = resolve_email(config, IdentRole::Committer)?;

    let date_str = std::env::var("GIT_COMMITTER_DATE").ok();
    let timestamp = match date_str {
        Some(d) => parse_date_to_git_timestamp(&d).unwrap_or(d),
        None => format_git_timestamp(now),
    };

    Ok(format!("{name} <{email}> {timestamp}"))
}

/// Parse a date string (like "2006-06-26 00:04:00 +0000") into git's
/// `<epoch> <offset>` format. Returns None if already in epoch format.
pub fn parse_date_to_git_timestamp(date_str: &str) -> Option<String> {
    let trimmed = date_str.trim();

    // Already in `<epoch> <offset>` format? (epoch is all digits)
    let parts: Vec<&str> = trimmed.rsplitn(2, ' ').collect();
    if parts.len() == 2 {
        let maybe_epoch = parts[1];
        if maybe_epoch.chars().all(|c| c.is_ascii_digit()) {
            // Already epoch + offset
            return None;
        }
    }

    // Try parsing "YYYY-MM-DD HH:MM:SS <tz>" format
    if parts.len() == 2 {
        let tz = parts[0];
        let datetime = parts[1];

        // Parse tz offset
        let tz_bytes = tz.as_bytes();
        if tz_bytes.len() >= 5 {
            let sign: i64 = if tz_bytes[0] == b'-' { -1 } else { 1 };
            let h: i64 = tz[1..3].parse().unwrap_or(0);
            let m: i64 = tz[3..5].parse().unwrap_or(0);
            let tz_secs = sign * (h * 3600 + m * 60);

            // Try YYYY-MM-DD HH:MM:SS
            if let Ok(offset) = time::UtcOffset::from_whole_seconds(tz_secs as i32) {
                let fmt = time::format_description::parse(
                    "[year]-[month]-[day] [hour]:[minute]:[second]",
                )
                .ok()?;
                if let Ok(naive) = time::PrimitiveDateTime::parse(datetime, &fmt) {
                    let dt = naive.assume_offset(offset);
                    let epoch = dt.unix_timestamp();
                    return Some(format!("{epoch} {tz}"));
                }
            }
        }
    }

    // Try "@<epoch>" format (git uses this for testing)
    if let Some(epoch_str) = trimmed.strip_prefix('@') {
        // @<epoch> <tz>
        let ep_parts: Vec<&str> = epoch_str.splitn(2, ' ').collect();
        if ep_parts.len() == 2 {
            if let Ok(_epoch) = ep_parts[0].parse::<i64>() {
                return Some(format!("{} {}", ep_parts[0], ep_parts[1]));
            }
        }
    }

    None
}

/// Format a timestamp in Git's format: `<epoch> <offset>`.
fn format_git_timestamp(dt: OffsetDateTime) -> String {
    let epoch = dt.unix_timestamp();
    let offset = dt.offset();
    let hours = offset.whole_hours();
    let minutes = offset.minutes_past_hour().unsigned_abs();
    format!("{epoch} {hours:+03}{minutes:02}")
}

/// Update HEAD to point to the new commit.
fn update_head(git_dir: &Path, head: &HeadState, commit_oid: &ObjectId) -> Result<()> {
    match head {
        HeadState::Branch { refname, .. } => {
            // Update the ref that HEAD points to
            if grit_lib::reftable::is_reftable_repo(git_dir) {
                grit_lib::reftable::reftable_write_ref(git_dir, refname, commit_oid, None, None)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            } else {
                let ref_path = git_dir.join(refname);
                if let Some(parent) = ref_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&ref_path, format!("{}\n", commit_oid.to_hex()))?;
            }
        }
        HeadState::Detached { .. } | HeadState::Invalid => {
            // Write directly to HEAD
            fs::write(git_dir.join("HEAD"), format!("{}\n", commit_oid.to_hex()))?;
        }
    }
    Ok(())
}

/// Clean up merge-related state files after a successful commit.
fn cleanup_merge_state(git_dir: &Path) {
    let _ = fs::remove_file(git_dir.join("MERGE_HEAD"));
    let _ = fs::remove_file(git_dir.join("MERGE_MSG"));
    let _ = fs::remove_file(git_dir.join("MERGE_MODE"));
    let _ = fs::remove_file(git_dir.join("SQUASH_MSG"));
    let _ = fs::remove_file(git_dir.join("CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(git_dir.join("REVERT_HEAD"));
}

/// Ensure a string ends with a newline.
fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}

fn is_permission_denied_error(err: &grit_lib::error::Error) -> bool {
    err.to_string().contains("Permission denied") || err.to_string().contains("permission denied")
}
