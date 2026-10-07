//! Deterministic synthetic repository generator (grit-lib backed).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use grit_lib::index::{Index, IndexEntry, MODE_REGULAR};
use grit_lib::objects::ObjectId;
use grit_lib::objects::{serialize_commit, serialize_tag, CommitData, ObjectKind, TagData};
use grit_lib::odb::Odb;
use grit_lib::pack_write::{install_pack_stream, whole_pack_uncompressed_size, InstallPackOptions};
use grit_lib::porcelain::checkout::checkout_between_trees;
use grit_lib::prune_packed::{prune_packed_objects, PrunePackedOptions};
use grit_lib::refs::{write_packed_refs_file, write_ref, PackedRefRecord};
use grit_lib::repo::{init_repository, Repository};
use grit_lib::transfer::{build_pack, PackBuildOptions};
use grit_lib::write_tree::{write_tree_from_index, write_tree_update_index, WriteTreeFlags};

/// Benchmark fixture size tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[value(rename_all = "uppercase")]
#[serde(rename_all = "UPPERCASE")]
pub enum SynthTier {
    /// CI-sized repo (~2k files, ~500 commits, ~1k refs).
    S,
    /// Medium repo (~20k files, ~5k commits, ~10k refs).
    M,
    /// Large repo (≥100k files, ≥50k commits, ≥100k refs, ≥1 GiB pack uncompressed).
    L,
}

/// Resolved numeric targets for a tier.
#[derive(Debug, Clone, Copy)]
pub struct TierSpec {
    pub file_count: usize,
    pub commit_count: usize,
    pub ref_count: usize,
    pub edits_per_commit: usize,
    pub blob_body_bytes: usize,
    pub loose_after_pack: usize,
    pub seed: u64,
}

impl SynthTier {
    /// Fixed tier parameters used by tests and the CLI.
    #[must_use]
    pub fn spec(self) -> TierSpec {
        match self {
            SynthTier::S => TierSpec {
                file_count: 2_000,
                commit_count: 500,
                ref_count: 1_000,
                edits_per_commit: 4,
                blob_body_bytes: 48,
                loose_after_pack: 8,
                seed: 0x312_0001,
            },
            SynthTier::M => TierSpec {
                file_count: 20_000,
                commit_count: 5_000,
                ref_count: 10_000,
                edits_per_commit: 8,
                blob_body_bytes: 64,
                loose_after_pack: 16,
                seed: 0x312_0002,
            },
            SynthTier::L => TierSpec {
                file_count: 100_000,
                commit_count: 50_000,
                ref_count: 100_000,
                edits_per_commit: 4,
                blob_body_bytes: 12_000,
                loose_after_pack: 32,
                seed: 0x312_0003,
            },
        }
    }
}

/// Summary returned after generation completes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SynthReport {
    pub tier: SynthTier,
    pub head: String,
    pub commit_count: usize,
    pub ref_count: usize,
    pub file_count: usize,
    pub pack_uncompressed_bytes: u64,
    pub generation_ms: u64,
    pub repo_path: PathBuf,
}

struct DetRng {
    state: u64,
}

impl DetRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn next_usize(&mut self, upper: usize) -> usize {
        if upper == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper
    }
}

/// Generate a deterministic synthetic repository at `out`.
pub fn generate_synth_repo(out: &Path, tier: SynthTier) -> Result<SynthReport> {
    let spec = tier.spec();
    let started = Instant::now();
    if out.exists() {
        fs::remove_dir_all(out).with_context(|| format!("clear {}", out.display()))?;
    }
    fs::create_dir_all(out).context("create output directory")?;
    let repo = init_repository(out, false, "main", None, "files").context("init repository")?;
    let head = generate_into_repo(&repo, spec)?;
    let pack_bytes = build_pack(
        &repo.odb,
        &[head],
        &[],
        &PackBuildOptions {
            delta: false,
            ..PackBuildOptions::default()
        },
    )
    .context("build pack")?;
    let pack_uncompressed =
        whole_pack_uncompressed_size(&pack_bytes, repo.odb.hash_algo()).context("measure pack")?;
    install_pack_stream(
        repo.odb.objects_dir(),
        &pack_bytes,
        InstallPackOptions {
            hash_algo: repo.odb.hash_algo(),
            ..InstallPackOptions::default()
        },
    )
    .context("install pack")?;
    let _ = prune_packed_objects(
        repo.odb.objects_dir(),
        PrunePackedOptions {
            quiet: true,
            ..PrunePackedOptions::default()
        },
    )?;
    materialize_loose_tip_objects(&repo, head, spec.loose_after_pack)?;
    let head_tree = commit_tree(&repo, head)?;
    checkout_between_trees(&repo, None, &head_tree).context("checkout worktree")?;
    let elapsed = started.elapsed();
    Ok(SynthReport {
        tier,
        head: head.to_string(),
        commit_count: spec.commit_count,
        ref_count: spec.ref_count,
        file_count: spec.file_count,
        pack_uncompressed_bytes: pack_uncompressed,
        generation_ms: elapsed.as_millis() as u64,
        repo_path: out.to_path_buf(),
    })
}

fn generate_into_repo(repo: &Repository, spec: TierSpec) -> Result<ObjectId> {
    let mut rng = DetRng::new(spec.seed);
    let paths = file_paths(spec.file_count);
    let mut file_oids: Vec<ObjectId> = Vec::with_capacity(paths.len());
    let mut index = Index::new();
    index.hash_algo = repo.odb.hash_algo();

    for (i, path) in paths.iter().enumerate() {
        let body = blob_body(i as u32, 0, spec.blob_body_bytes);
        let oid = repo
            .odb
            .write(ObjectKind::Blob, &body)
            .context("write blob")?;
        file_oids.push(oid);
        index.add_or_replace(synth_index_entry(path, oid, body.len()));
    }

    let mut parent: Option<ObjectId> = None;
    let mut commits: Vec<ObjectId> = Vec::with_capacity(spec.commit_count);
    for commit_idx in 0..spec.commit_count {
        if commit_idx > 0 {
            for _ in 0..spec.edits_per_commit {
                let fi = rng.next_usize(paths.len());
                let body = blob_body(fi as u32, commit_idx as u32, spec.blob_body_bytes);
                let oid = repo
                    .odb
                    .write(ObjectKind::Blob, &body)
                    .context("write blob")?;
                file_oids[fi] = oid;
                index.add_or_replace(synth_index_entry(&paths[fi], oid, body.len()));
            }
        }
        let tree = if commit_idx == 0 {
            write_tree_from_index(&repo.odb, &index, "").context("write tree")?
        } else {
            write_tree_update_index(&repo.odb, &mut index, "", WriteTreeFlags::default())
                .context("update tree")?
        };
        let ts = 1_700_000_000i64 + commit_idx as i64;
        let ident = format!("Synth Generator <synth@grit-scm.com> {ts} +0000");
        let commit = CommitData {
            tree,
            parents: parent.into_iter().collect(),
            author: ident.clone(),
            committer: ident,
            author_raw: Vec::new(),
            committer_raw: Vec::new(),
            encoding: None,
            message: format!("synthetic commit {commit_idx}\n"),
            raw_message: None,
        };
        let raw = serialize_commit(&commit);
        let commit_oid = repo
            .odb
            .write(ObjectKind::Commit, &raw)
            .context("write commit")?;
        commits.push(commit_oid);
        parent = Some(commit_oid);
    }
    let head = *commits.last().context("no commits")?;
    write_refs(repo, &commits, spec, &mut rng)?;
    write_ref(&repo.git_dir, "refs/heads/main", &head).context("write main")?;
    Ok(head)
}

fn write_refs(
    repo: &Repository,
    commits: &[ObjectId],
    spec: TierSpec,
    rng: &mut DetRng,
) -> Result<()> {
    let branch_refs = spec.ref_count * 2 / 3;
    let tag_refs = spec.ref_count - branch_refs - 1; // minus main written loose
    let mut records = Vec::with_capacity(spec.ref_count);
    for i in 0..branch_refs {
        let name = format!("refs/heads/synth/b{i:06}");
        let oid = commits[i % commits.len()];
        records.push(PackedRefRecord {
            refname: name,
            oid,
            peeled: None,
        });
    }
    for i in 0..tag_refs {
        let commit = commits[(i * 17 + rng.next_usize(commits.len())) % commits.len()];
        if i % 2 == 0 {
            let tag_name = format!("v{i:06}");
            let ts = 1_700_100_000i64 + i as i64;
            let tagger = format!("Synth Generator <synth@grit-scm.com> {ts} +0000");
            let tag = TagData {
                object: commit,
                object_type: "commit".to_owned(),
                tag: tag_name.clone(),
                tagger: Some(tagger),
                message: format!("annotated {tag_name}\n"),
            };
            let raw = serialize_tag(&tag);
            let tag_oid = repo.odb.write(ObjectKind::Tag, &raw).context("write tag")?;
            records.push(PackedRefRecord {
                refname: format!("refs/tags/{tag_name}"),
                oid: tag_oid,
                peeled: Some(commit),
            });
        } else {
            records.push(PackedRefRecord {
                refname: format!("refs/tags/light-{i:06}"),
                oid: commit,
                peeled: None,
            });
        }
    }
    write_packed_refs_file(&repo.git_dir, &records).context("write packed-refs")?;
    Ok(())
}

fn materialize_loose_tip_objects(repo: &Repository, head: ObjectId, count: usize) -> Result<()> {
    let head_obj = repo.odb.read(&head).context("read head")?;
    let commit = grit_lib::objects::parse_commit(&head_obj.data).context("parse head")?;
    let mut blobs = Vec::new();
    collect_blob_oids(&repo.odb, &commit.tree, &mut blobs)?;
    for oid in blobs.into_iter().take(count) {
        let obj = repo.odb.read(&oid).context("read blob")?;
        repo.odb
            .write_loose_materialize(obj.kind, &obj.data)
            .context("materialize loose")?;
    }
    Ok(())
}

fn collect_blob_oids(odb: &Odb, tree_oid: &ObjectId, out: &mut Vec<ObjectId>) -> Result<()> {
    use grit_lib::index::MODE_TREE;
    let tree_obj = odb.read(tree_oid).context("read tree")?;
    for entry in grit_lib::objects::parse_tree(&tree_obj.data).context("parse tree")? {
        if entry.mode == MODE_TREE {
            collect_blob_oids(odb, &entry.oid, out)?;
        } else {
            out.push(entry.oid);
        }
    }
    Ok(())
}

fn commit_tree(repo: &Repository, head: ObjectId) -> Result<ObjectId> {
    let head_obj = repo.odb.read(&head)?;
    let commit = grit_lib::objects::parse_commit(&head_obj.data)?;
    Ok(commit.tree)
}

fn synth_index_entry(path: &[u8], oid: ObjectId, size: usize) -> IndexEntry {
    IndexEntry {
        ctime_sec: 1_700_000_000,
        ctime_nsec: 0,
        mtime_sec: 1_700_000_000,
        mtime_nsec: 0,
        dev: 0,
        ino: 0,
        mode: MODE_REGULAR,
        uid: 0,
        gid: 0,
        size: size as u32,
        oid,
        flags: (path.len().min(0xfff)) as u16,
        flags_extended: None,
        path: path.to_vec(),
        base_index_pos: 0,
    }
}

fn file_paths(count: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let dir = i / 100;
        let file = i % 100;
        out.push(format!("data/d{dir:04}/f{file:04}.txt").into_bytes());
    }
    out
}

fn blob_body(file_id: u32, version: u32, body_bytes: usize) -> Vec<u8> {
    let mut s = format!("synth file={file_id} ver={version}\n");
    if s.len() < body_bytes {
        s.push_str(&"x".repeat(body_bytes - s.len()));
    }
    s.into_bytes()
}

/// Validate tier **S** output using the system `git` binary (integration tests).
pub fn verify_tier_s_with_git(repo: &Path, spec: TierSpec) -> Result<()> {
    let git = crate::binary::resolve_binary("git", None)?;
    run_git_ok(&git, repo, &["fsck", "--strict", "--no-dangling"])?;
    let count = git_output(&git, repo, &["rev-list", "--count", "HEAD"])?;
    if count.trim() != spec.commit_count.to_string() {
        bail!("commit count {} != {}", count.trim(), spec.commit_count);
    }
    let refs = git_output(&git, repo, &["for-each-ref"])?;
    let ref_lines = refs.lines().filter(|l| !l.is_empty()).count();
    if ref_lines != spec.ref_count {
        bail!("ref count {ref_lines} != {}", spec.ref_count);
    }
    let status = git_output(&git, repo, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        bail!("non-clean status:\n{status}");
    }
    Ok(())
}

fn run_git_ok(git: &Path, dir: &Path, args: &[&str]) -> Result<()> {
    let out = std::process::Command::new(git)
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

fn git_output(git: &Path, dir: &Path, args: &[&str]) -> Result<String> {
    run_git_ok(git, dir, args)?;
    let out = std::process::Command::new(git)
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("git {}", args.join(" ")))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
