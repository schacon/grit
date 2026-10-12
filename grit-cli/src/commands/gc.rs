//! `grit gc` — repack and prune unreachable objects.

use std::io::Write;

use anyhow::Result;
use grit_lib::maintenance::{gc, GcOptions, GcStats, RepackOptions};
use schemars::JsonSchema;
use serde::Serialize;
use time::OffsetDateTime;

use crate::context;
use crate::output::{outln, HumanRender, MarkdownRender, RenderContext};

/// Result of `grit gc`.
#[derive(Serialize, JsonSchema)]
pub struct GcOutcome {
    #[serde(skip)]
    pub quiet: bool,
    pub dry_run: bool,
    pub reflog_entries_pruned: usize,
    pub repack: RepackSummary,
    pub prune: PruneSummary,
}

#[derive(Serialize, JsonSchema)]
pub struct RepackSummary {
    pub loose_before: usize,
    pub packs_before: usize,
    pub objects_packed: Option<usize>,
    pub pack_bytes: Option<u64>,
    pub pack_path: Option<String>,
    pub removed_pack_files: usize,
    pub pruned_loose: usize,
}

#[derive(Serialize, JsonSchema)]
pub struct PruneSummary {
    pub pruned: usize,
    pub kept: usize,
}

impl From<(GcStats, bool, bool)> for GcOutcome {
    fn from((stats, dry_run, quiet): (GcStats, bool, bool)) -> Self {
        Self {
            quiet,
            dry_run,
            reflog_entries_pruned: stats.reflog_entries_pruned,
            repack: RepackSummary {
                loose_before: stats.repack.loose_before,
                packs_before: stats.repack.packs_before,
                objects_packed: stats.repack.objects_packed,
                pack_bytes: stats.repack.pack_bytes,
                pack_path: stats.repack.pack_path.map(|p| p.display().to_string()),
                removed_pack_files: stats.repack.removed_pack_files,
                pruned_loose: stats.repack.pruned_loose,
            },
            prune: PruneSummary {
                pruned: stats.prune.pruned,
                kept: stats.prune.kept,
            },
        }
    }
}

impl HumanRender for GcOutcome {
    fn render_human(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        if self.quiet {
            return Ok(());
        }
        if self.dry_run {
            outln!(w, "Dry run: no changes written.");
            if self.reflog_entries_pruned > 0 {
                outln!(
                    w,
                    "Would prune {} reflog entries.",
                    self.reflog_entries_pruned
                );
            }
            if let Some(n) = self.repack.objects_packed {
                outln!(w, "Would pack {n} reachable objects.");
            }
            return Ok(());
        }
        if let Some(path) = &self.repack.pack_path {
            outln!(
                w,
                "Packed {} objects into {}.",
                self.repack.objects_packed.unwrap_or(0),
                path
            );
        }
        if self.prune.pruned > 0 {
            outln!(
                w,
                "Pruned {} unreachable loose objects ({} kept).",
                self.prune.pruned,
                self.prune.kept
            );
        } else {
            outln!(w, "No unreachable loose objects to prune.");
        }
        Ok(())
    }
}

impl MarkdownRender for GcOutcome {
    fn render_markdown(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        outln!(w, "## Garbage collection\n");
        outln!(w, "- **dry_run:** {}", self.dry_run);
        outln!(
            w,
            "- **reflog_entries_pruned:** {}",
            self.reflog_entries_pruned
        );
        outln!(w, "\n### Repack\n");
        outln!(w, "- **objects_packed:** {:?}", self.repack.objects_packed);
        outln!(w, "- **pack_path:** {:?}", self.repack.pack_path);
        outln!(w, "\n### Prune\n");
        outln!(w, "- **pruned:** {}", self.prune.pruned);
        outln!(w, "- **kept:** {}", self.prune.kept);
        Ok(())
    }
}

pub fn run(dry_run: bool, quiet: bool) -> Result<GcOutcome> {
    let repo = context::discover()?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let stats = gc(
        &repo,
        &GcOptions {
            repack: RepackOptions {
                dry_run,
                ..RepackOptions::default()
            },
            expire_reflog: true,
            now_unix: now,
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok((stats, dry_run, quiet).into())
}
