//! `grit log` — the recent history reachable from HEAD.
//!
//! Deliberately minimal: it shows one page of commits (newest first) and, when
//! there's more, prints the command to fetch the next page.

use anyhow::{Context, Result};
use grit_lib::revwalk::log_walk::LogWalkCursor;
use schemars::JsonSchema;
use serde::Serialize;
use std::io::Write;

use crate::context::{self, subject_line, CommitSummary};
use crate::markdown;
use crate::output::{outln, CommitJson, HumanRender, MarkdownRender, RenderContext};
use crate::ui;

/// How many commits to show per page.
const PAGE: usize = 10;

/// Result of `grit log`: one page of commits, plus the next page's start (if any).
#[derive(Serialize, JsonSchema)]
pub struct LogOutcome {
    pub commits: Vec<CommitJson>,
    /// Opaque continuation cursor for `--before`, or `null` when there is no further history.
    pub next: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    commit_rows: Vec<CommitSummary>,
}

impl HumanRender for LogOutcome {
    fn render_human(&self, w: &mut dyn Write, ctx: &RenderContext) -> Result<()> {
        if self.commits.is_empty() {
            outln!(w, "No commits yet.");
            return Ok(());
        }
        let now = ctx.now.unix_timestamp();
        for row in ui::commit_rows(&self.commit_rows, now) {
            outln!(w, "{row}");
        }
        if let Some(next) = &self.next {
            outln!(w);
            outln!(w, "→ more: grit log --before={next}");
        }
        Ok(())
    }
}

impl MarkdownRender for LogOutcome {
    fn render_markdown(&self, w: &mut dyn Write, ctx: &RenderContext) -> Result<()> {
        if self.commit_rows.is_empty() {
            outln!(w, "No commits yet.");
            return Ok(());
        }
        markdown::print_commit_list(w, &self.commit_rows, ctx)?;
        if let Some(next) = &self.next {
            outln!(w);
            outln!(w, "More history: run `grit log --before={next}`");
        }
        Ok(())
    }
}

pub fn run(before: Option<String>) -> Result<LogOutcome> {
    let repo = context::discover()?;
    let cursor = before
        .as_deref()
        .map(LogWalkCursor::decode)
        .transpose()
        .context("invalid log continuation cursor")?;

    let page =
        LogWalkCursor::step(&repo, cursor.as_ref(), PAGE).context("could not list commits")?;

    let env = repo.environment();
    let now = context::wall_clock_now(env).unix_timestamp();

    let commit_rows = page
        .commits
        .iter()
        .map(|oid| {
            let commit = context::read_commit(&repo, oid)?;
            let (author, timestamp) = context::author_and_time(&commit.author);
            Ok(CommitSummary {
                oid: *oid,
                subject: subject_line(&commit.message),
                author,
                timestamp,
                author_date: crate::dates::rfc3339_from_identity_when(
                    crate::dates::identity_when_suffix(&commit.author),
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let commits = commit_rows
        .iter()
        .map(|row| CommitJson::from_summary(row, now))
        .collect();

    let next = page.next.map(|c| c.encode()).transpose()?;

    Ok(LogOutcome {
        commits,
        next,
        commit_rows,
    })
}
