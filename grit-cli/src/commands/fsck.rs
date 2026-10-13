//! `grit fsck` — check repository integrity.

use std::io::Write;

use anyhow::Result;
use grit_lib::maintenance::{fsck, FsckOptions, FsckReport, FsckSeverity};
use schemars::JsonSchema;
use serde::Serialize;

use crate::context;
use crate::output::{outln, HumanRender, MarkdownRender, RenderContext};

/// Result of `grit fsck` / `grit verify`.
#[derive(Serialize, JsonSchema)]
pub struct FsckOutcome {
    pub ok: bool,
    pub issues: Vec<FsckIssueJson>,
}

#[derive(Serialize, JsonSchema)]
pub struct FsckIssueJson {
    pub severity: String,
    pub path: String,
    pub msg_id: String,
    pub detail: String,
}

impl From<FsckReport> for FsckOutcome {
    fn from(report: FsckReport) -> Self {
        Self {
            ok: report.ok,
            issues: report
                .issues
                .into_iter()
                .map(|i| FsckIssueJson {
                    severity: match i.severity {
                        FsckSeverity::Error => "error".to_owned(),
                        FsckSeverity::Warning => "warning".to_owned(),
                    },
                    path: i.path,
                    msg_id: i.msg_id,
                    detail: i.detail,
                })
                .collect(),
        }
    }
}

impl HumanRender for FsckOutcome {
    fn render_human(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        if self.ok && self.issues.is_empty() {
            outln!(w, "Repository integrity check passed.");
            return Ok(());
        }
        for issue in &self.issues {
            let line = format!(
                "{}: {}: {}: {}",
                issue.severity, issue.path, issue.msg_id, issue.detail
            );
            if issue.severity == "error" {
                outln!(w, "{line}");
            } else {
                outln!(w, "{line}");
            }
        }
        if self.ok {
            outln!(w, "Completed with warnings.");
        } else {
            outln!(w, "Integrity check failed.");
        }
        Ok(())
    }
}

impl MarkdownRender for FsckOutcome {
    fn render_markdown(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        outln!(w, "## Fsck\n");
        outln!(w, "- **ok:** {}", self.ok);
        if self.issues.is_empty() {
            outln!(w, "\nNo issues reported.");
            return Ok(());
        }
        outln!(w, "\n| Severity | Path | Msg ID | Detail |");
        outln!(w, "| --- | --- | --- | --- |");
        for issue in &self.issues {
            outln!(
                w,
                "| {} | `{}` | `{}` | {} |",
                issue.severity,
                issue.path,
                issue.msg_id,
                issue.detail
            );
        }
        Ok(())
    }
}

pub fn run(strict: bool) -> Result<FsckOutcome> {
    let repo = context::discover()?;
    let report =
        fsck(&repo, &FsckOptions::full_repository(strict)).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(report.into())
}

/// Exit code helper: non-zero when errors were found.
pub fn exit_code(outcome: &FsckOutcome) -> i32 {
    if outcome.ok {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_matches_library() {
        let issue = grit_lib::maintenance::FsckIssue {
            severity: FsckSeverity::Error,
            path: "abcd".into(),
            msg_id: "missingObject".into(),
            detail: "gone".into(),
        };
        let line = grit_lib::maintenance::format_fsck_line(&issue);
        assert!(line.contains("missingObject"));
    }
}
