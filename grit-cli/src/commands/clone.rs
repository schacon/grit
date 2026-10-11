//! `grit clone` — copy a remote repository into a new directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use grit_lib::clone::{
    cleanup_failed_clone, clone as clone_repo, derive_clone_dir, dest_preexisted_empty, CloneError,
    CloneOptions,
};
use grit_lib::porcelain::checkout::checkout_between_trees_with_progress;
use grit_lib::remote::DefaultHttpClientFactory;
use serde::Serialize;

use crate::context;
use crate::output::{progress, HumanRender, MarkdownRender, OutputMode};
use crate::progress::TerminalProgress;

/// Result of `grit clone`.
#[derive(Serialize)]
pub struct CloneOutcome {
    pub url: String,
    /// Destination directory.
    pub path: String,
    /// Default branch checked out.
    pub branch: String,
}

impl HumanRender for CloneOutcome {
    fn render_human(&self) {
        println!("Cloned into '{}' on branch {}.", self.path, self.branch);
    }
}

impl MarkdownRender for CloneOutcome {}

pub fn run(url: &str, dir: Option<String>, mode: OutputMode) -> Result<CloneOutcome> {
    let dir = dir.unwrap_or_else(|| derive_clone_dir(url));
    let path = PathBuf::from(&dir);
    let dest_preexisted_empty = dest_preexisted_empty(&path);

    progress(mode, &format!("Cloning into '{dir}' ..."));
    match clone_into(url, &path, &dir, mode) {
        Ok(outcome) => Ok(outcome),
        Err(err) => {
            let skip_cleanup = matches!(
                err.downcast_ref::<CloneError>(),
                Some(CloneError::DestNotEmpty(_))
            );
            if !skip_cleanup {
                cleanup_failed_clone(&path, dest_preexisted_empty);
            }
            Err(err)
        }
    }
}

fn clone_into(url: &str, path: &Path, dir: &str, mode: OutputMode) -> Result<CloneOutcome> {
    let factory = DefaultHttpClientFactory;
    let clone_opts = CloneOptions {
        url: url.to_owned(),
        dest: path.to_path_buf(),
        environment: crate::context::environment(),
        remote_name: grit_lib::remote::DEFAULT_REMOTE.to_owned(),
        initial_branch: "main".to_owned(),
        thread_policy: crate::context::thread_policy_for_repo(),
    };
    let mut term_progress = TerminalProgress::new(mode);
    let outcome = clone_repo(clone_opts, &mut term_progress, Some(&factory))?;

    let tree = context::commit_tree(&outcome.repo, &outcome.checkout_oid)?;
    let progress_enabled =
        mode == OutputMode::Human && crate::terminal_progress::stderr_progress_enabled();
    let mut progress = crate::terminal_progress::TerminalProgress::new(progress_enabled);
    checkout_between_trees_with_progress(&outcome.repo, None, &tree, &mut progress)
        .context("could not check out files")?;

    Ok(CloneOutcome {
        url: url.to_owned(),
        path: dir.to_owned(),
        branch: outcome.branch,
    })
}
