//! `grit cat` — show object contents (blob, tree, commit, or tag).

use std::io::Write;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use grit_lib::object_inspect::ResolveError;
use grit_lib::objects::{Object, ObjectKind, TreeEntry};
use grit_lib::repo::Repository;
use schemars::JsonSchema;
use serde::Serialize;

use crate::context;
use crate::output::{outln, HumanRender, MarkdownRender, RenderContext};

/// Blob payload in JSON (`--json` contract).
#[derive(Serialize, JsonSchema)]
pub struct BlobJson {
    pub encoding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
}

/// One tree line in JSON (same fields as human `ls-tree` output).
#[derive(Serialize, JsonSchema)]
pub struct TreeLineJson {
    pub mode: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub oid: String,
    pub path: String,
}

/// Commit or tag body in JSON (canonical object payload text).
#[derive(Serialize, JsonSchema)]
pub struct TextObjectJson {
    pub text: String,
}

/// Object payload shape in `--json` output.
#[derive(Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatContentJson {
    Blob(BlobJson),
    Tree { entries: Vec<TreeLineJson> },
    Commit(TextObjectJson),
    Tag(TextObjectJson),
}

/// Result of `grit cat`.
#[derive(Serialize, JsonSchema)]
pub struct CatOutcome {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub warnings: Vec<crate::diagnostics::WarningRecord>,
    pub spec: String,
    pub oid: String,
    pub kind: String,
    pub size: u64,
    pub content: CatContentJson,
    /// Raw bytes for human blob output (omitted from JSON).
    #[serde(skip)]
    #[schemars(skip)]
    pub human_blob: Option<Vec<u8>>,
    /// Preformatted human text for tree/commit/tag (omitted from JSON).
    #[serde(skip)]
    #[schemars(skip)]
    pub human_text: Option<String>,
}

impl HumanRender for CatOutcome {
    fn render_human(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        if let Some(bytes) = &self.human_blob {
            w.write_all(bytes)
                .map_err(|e| anyhow::anyhow!("writing blob: {e}"))?;
            return Ok(());
        }
        if let Some(text) = &self.human_text {
            w.write_all(text.as_bytes())
                .map_err(|e| anyhow::anyhow!("writing object: {e}"))?;
        }
        Ok(())
    }
}

impl MarkdownRender for CatOutcome {
    fn render_markdown(&self, w: &mut dyn Write, _ctx: &RenderContext) -> Result<()> {
        outln!(w, "## Cat\n");
        outln!(w, "- **spec:** `{}`", self.spec);
        outln!(w, "- **oid:** `{}`", self.oid);
        outln!(w, "- **kind:** {}", self.kind);
        outln!(w, "- **size:** {}", self.size);
        outln!(w);
        match &self.content {
            CatContentJson::Blob(b) => {
                if let Some(text) = &b.text {
                    outln!(w, "```text");
                    outln!(w, "{text}");
                    outln!(w, "```");
                } else {
                    outln!(w, "_binary blob (base64 in JSON)_");
                }
            }
            CatContentJson::Tree { entries } => {
                outln!(w, "| Mode | Type | OID | Path |");
                outln!(w, "| --- | --- | --- | --- |");
                for e in entries {
                    outln!(
                        w,
                        "| {} | {} | `{}` | `{}` |",
                        e.mode,
                        e.object_type,
                        e.oid,
                        e.path.replace('|', "\\|")
                    );
                }
            }
            CatContentJson::Commit(t) | CatContentJson::Tag(t) => {
                outln!(w, "```text");
                outln!(w, "{}", t.text.trim_end());
                outln!(w, "```");
            }
        }
        Ok(())
    }
}

pub fn run(spec: String) -> Result<CatOutcome> {
    let (repo, sink) = context::discover_with_warnings()?;
    let resolved = repo
        .resolve(&spec)
        .map_err(|e| map_resolve_error(&spec, e))?;
    let obj = repo
        .odb()
        .read(&resolved.oid)
        .with_context(|| format!("read object {}", resolved.oid.to_hex()))?;
    let (content, human_blob, human_text) = build_content(&repo, &obj)?;
    Ok(CatOutcome {
        warnings: sink.warnings(),
        spec,
        oid: resolved.oid.to_hex(),
        kind: kind_label(obj.kind).to_owned(),
        size: resolved.size,
        content,
        human_blob,
        human_text,
    })
}

fn build_content(
    repo: &Repository,
    obj: &Object,
) -> Result<(CatContentJson, Option<Vec<u8>>, Option<String>)> {
    match obj.kind {
        ObjectKind::Blob => {
            let json = blob_json(&obj.data);
            Ok((CatContentJson::Blob(json), Some(obj.data.clone()), None))
        }
        ObjectKind::Tree => {
            let parsed = grit_lib::objects::parse_tree(&obj.data)?;
            let lines = tree_lines(repo, &parsed)?;
            let human = format_tree_human(&lines);
            let entries = lines
                .into_iter()
                .map(|l| TreeLineJson {
                    mode: l.mode,
                    object_type: l.object_type,
                    oid: l.oid,
                    path: l.path,
                })
                .collect();
            Ok((CatContentJson::Tree { entries }, None, Some(human)))
        }
        ObjectKind::Commit | ObjectKind::Tag => {
            let text = String::from_utf8_lossy(&obj.data).into_owned();
            let json = match obj.kind {
                ObjectKind::Commit => CatContentJson::Commit(TextObjectJson { text: text.clone() }),
                ObjectKind::Tag => CatContentJson::Tag(TextObjectJson { text: text.clone() }),
                _ => unreachable!(),
            };
            Ok((json, None, Some(text)))
        }
    }
}

struct TreeLine {
    mode: String,
    object_type: String,
    oid: String,
    path: String,
}

fn tree_lines(repo: &Repository, entries: &[TreeEntry]) -> Result<Vec<TreeLine>> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let info = repo.odb().read_info(&e.oid).ok();
        let object_type = info
            .map(|i| kind_label(i.kind).to_owned())
            .unwrap_or_else(|| "blob".to_owned());
        let path = String::from_utf8_lossy(&e.name).into_owned();
        out.push(TreeLine {
            mode: e.mode_str(),
            object_type,
            oid: e.oid.to_hex(),
            path,
        });
    }
    Ok(out)
}

fn format_tree_human(lines: &[TreeLine]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&format!(
            "{} {} {}\t{}\n",
            line.mode, line.object_type, line.oid, line.path
        ));
    }
    out
}

fn blob_json(data: &[u8]) -> BlobJson {
    if data.is_empty() || std::str::from_utf8(data).is_ok() {
        BlobJson {
            encoding: "utf-8".to_owned(),
            text: Some(String::from_utf8_lossy(data).into_owned()),
            data_base64: None,
        }
    } else {
        BlobJson {
            encoding: "base64".to_owned(),
            text: None,
            data_base64: Some(STANDARD.encode(data)),
        }
    }
}

fn kind_label(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Blob => "blob",
        ObjectKind::Tree => "tree",
        ObjectKind::Commit => "commit",
        ObjectKind::Tag => "tag",
    }
}

fn map_resolve_error(spec: &str, err: ResolveError) -> anyhow::Error {
    match err {
        ResolveError::AmbiguousShortObject { candidates, .. } => {
            let mut msg = format!("ambiguous short object id '{spec}'");
            for hint in candidates {
                let kind = hint.kind.unwrap_or("object");
                msg.push_str(&format!("\n  {} {}", kind, hint.oid_hex));
            }
            anyhow::anyhow!(msg)
        }
        ResolveError::PathNotInTree { path, revision } => {
            anyhow::anyhow!("path '{path}' does not exist in '{revision}'")
        }
        ResolveError::UnknownRevision { .. } => {
            anyhow::anyhow!("unknown revision '{spec}'")
        }
        _ => anyhow::anyhow!("could not resolve '{spec}'"),
    }
}
