//! `grit pack-redundant` — find redundant pack files.
//!
//! Matches upstream `git pack-redundant` behavior (see `git/builtin/pack-redundant.c`):
//! computes a minimal cover of objects, then prints `.idx` and `.pack` paths for packs
//! not in that cover.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::objects::ObjectId;
use grit_lib::pack::{read_alternates_recursive, read_local_pack_indexes, PackIndex};
use grit_lib::repo::Repository;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, IsTerminal};
use std::path::PathBuf;

/// Arguments for `grit pack-redundant`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Also consider alternate object databases.
    #[arg(long)]
    pub alt_odb: bool,

    /// Consider all pack files in the repository.
    #[arg(long)]
    pub all: bool,

    /// Acknowledge deprecated command (required to run).
    #[arg(long, hide = true)]
    pub i_still_use_this: bool,

    /// Verbose output (to stderr).
    #[arg(short, long)]
    pub verbose: bool,
}

struct PackState {
    pack_path: PathBuf,
    idx_path: PathBuf,
    /// Objects remaining after alternates / ignore processing (sorted unique).
    remaining: BTreeSet<ObjectId>,
    all_objects_size: usize,
    /// Objects unique to this pack vs other *local* packs (after cmp_local_packs).
    unique: Option<BTreeSet<ObjectId>>,
}

/// Run `grit pack-redundant`.
pub fn run(args: Args) -> Result<()> {
    if !args.i_still_use_this {
        eprintln!("'git pack-redundant' is nominated for removal.\n");
        bail!("fatal: refusing to run without --i-still-use-this");
    }

    let repo = Repository::discover(None).context("not a git repository")?;
    let objects_dir = repo.odb.objects_dir();

    let mut local_indexes = read_local_pack_indexes(objects_dir).context("reading pack indexes")?;

    let mut alt_indexes: Vec<PackIndex> = Vec::new();
    if args.alt_odb || args.verbose {
        let alts = read_alternates_recursive(objects_dir).unwrap_or_default();
        for alt in alts {
            if let Ok(mut idxs) = read_local_pack_indexes(&alt) {
                alt_indexes.append(&mut idxs);
            }
        }
    }

    if !args.all {
        bail!("usage: git pack-redundant [--verbose] [--alt-odb] (--all | <pack-filename>...)");
    }

    if local_indexes.is_empty() {
        bail!("fatal: Zero packs found!");
    }

    let mut local_packs: Vec<PackState> = local_indexes
        .drain(..)
        .map(|idx| {
            let all_objects_size = idx.entries.len();
            let remaining: BTreeSet<ObjectId> = idx.entries.iter().map(|e| e.oid).collect();
            PackState {
                idx_path: idx.idx_path,
                pack_path: idx.pack_path,
                remaining,
                all_objects_size,
                unique: None,
            }
        })
        .collect();

    let alt_remaining: Vec<BTreeSet<ObjectId>> = alt_indexes
        .into_iter()
        .map(|idx| idx.entries.iter().map(|e| e.oid).collect())
        .collect();

    // Union of objects in local packs, minus objects only in alt odb (matches load_all_objects).
    let mut all_objects: BTreeSet<ObjectId> = BTreeSet::new();
    for p in &local_packs {
        all_objects.extend(p.remaining.iter().copied());
    }
    for alt_set in &alt_remaining {
        all_objects = sorted_difference(&all_objects, alt_set);
    }

    if args.alt_odb {
        for local in &mut local_packs {
            let mut rem = local.remaining.clone();
            for alt_set in &alt_remaining {
                rem = sorted_difference(&rem, alt_set);
            }
            local.remaining = rem;
        }
    }

    let ignore = read_ignore_oids()?;
    if !ignore.is_empty() {
        all_objects = sorted_difference(&all_objects, &ignore);
        for local in &mut local_packs {
            local.remaining = sorted_difference(&local.remaining, &ignore);
        }
    }

    cmp_local_packs(&mut local_packs);

    let min_set = minimize(&local_packs);
    let min_paths: std::collections::HashSet<PathBuf> =
        min_set.iter().map(|p| p.pack_path.clone()).collect();

    if args.verbose {
        let alt_count = alt_remaining.len();
        eprintln!("There are {alt_count} packs available in alt-odbs.\n");
        eprintln!("The smallest (bytewise) set of packs is:");
        for p in &min_set {
            eprintln!("\t{}", p.pack_path.display());
        }
        let dup = pack_redundancy_count(&min_set);
        let bytes = pack_set_bytecount(&min_set);
        eprintln!(
            "containing {dup} duplicate objects with a total size of {}kb.\n",
            bytes / 1024
        );
        eprintln!(
            "A total of {} unique objects were considered.\n",
            all_objects.len()
        );
        eprintln!("Redundant packs (with indexes):");
    }

    let mut redundant: Vec<&PackState> = local_packs
        .iter()
        .filter(|p| !min_paths.contains(&p.pack_path))
        .collect();

    redundant.sort_by_key(|p| p.pack_path.as_path());

    let red_bytecount: u64 = redundant
        .iter()
        .map(|p| pack_file_sizes(p))
        .fold(0u64, |a, (pack, idx)| a + pack + idx);

    for p in redundant {
        println!("{}", p.idx_path.display());
        println!("{}", p.pack_path.display());
    }

    if args.verbose {
        eprintln!(
            "{}MB of redundant packs in total.\n",
            red_bytecount / (1024 * 1024)
        );
    }

    Ok(())
}

fn pack_file_sizes(p: &PackState) -> (u64, u64) {
    let pack = fs::metadata(&p.pack_path).map(|m| m.len()).unwrap_or(0);
    let idx = fs::metadata(&p.idx_path).map(|m| m.len()).unwrap_or(0);
    (pack, idx)
}

fn pack_set_bytecount(packs: &[&PackState]) -> u64 {
    packs
        .iter()
        .map(|p| {
            let (a, b) = pack_file_sizes(p);
            a + b
        })
        .sum()
}

fn sizeof_union_entries(a: &PackIndex, b: &PackIndex) -> usize {
    let mut i = 0usize;
    let mut j = 0usize;
    let ea = &a.entries;
    let eb = &b.entries;
    let mut count = 0usize;
    while i < ea.len() && j < eb.len() {
        match ea[i].oid.cmp(&eb[j].oid) {
            std::cmp::Ordering::Equal => {
                count += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    count
}

fn pack_redundancy_count(min_set: &[&PackState]) -> usize {
    // Re-load indexes for pair walks (small n in tests).
    let mut indexes = Vec::new();
    for p in min_set {
        if let Ok(idx) = grit_lib::pack::read_pack_index(&p.idx_path) {
            indexes.push(idx);
        }
    }
    let mut total = 0usize;
    for i in 0..indexes.len() {
        for j in (i + 1)..indexes.len() {
            total += sizeof_union_entries(&indexes[i], &indexes[j]);
        }
    }
    total
}

fn read_ignore_oids() -> Result<BTreeSet<ObjectId>> {
    let mut out = BTreeSet::new();
    if io::stdin().is_terminal() {
        return Ok(out);
    }
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    let mut line = String::new();
    while lock.read_line(&mut line)? > 0 {
        let t = line.trim();
        if t.is_empty() {
            line.clear();
            continue;
        }
        let oid = ObjectId::from_hex(t)
            .map_err(|_| anyhow::anyhow!("fatal: Bad object ID on stdin: {t}"))?;
        out.insert(oid);
        line.clear();
    }
    Ok(out)
}

fn sorted_difference(a: &BTreeSet<ObjectId>, b: &BTreeSet<ObjectId>) -> BTreeSet<ObjectId> {
    a.difference(b).copied().collect()
}

fn cmp_two_packs(a: &mut PackState, b: &mut PackState) {
    if a.unique.is_none() {
        a.unique = Some(a.remaining.clone());
    }
    if b.unique.is_none() {
        b.unique = Some(b.remaining.clone());
    }
    let ua = a.unique.as_mut().expect("unique set");
    let ub = b.unique.as_mut().expect("unique set");

    let va: Vec<ObjectId> = ua.iter().copied().collect();
    let vb: Vec<ObjectId> = ub.iter().copied().collect();
    let mut ia = 0usize;
    let mut ib = 0usize;
    while ia < va.len() && ib < vb.len() {
        match va[ia].cmp(&vb[ib]) {
            std::cmp::Ordering::Equal => {
                ua.remove(&va[ia]);
                ub.remove(&vb[ib]);
                ia += 1;
                ib += 1;
            }
            std::cmp::Ordering::Less => ia += 1,
            std::cmp::Ordering::Greater => ib += 1,
        }
    }
}

fn cmp_local_packs(packs: &mut [PackState]) {
    if packs.len() < 2 {
        for p in packs {
            if p.unique.is_none() {
                p.unique = Some(BTreeSet::new());
            }
        }
        return;
    }
    let n = packs.len();
    for i in 0..n {
        for j in (i + 1)..n {
            // cmp_two_packs needs disjoint borrows.
            let (left, right) = packs.split_at_mut(j);
            cmp_two_packs(&mut left[i], &mut right[0]);
        }
    }
}

fn minimize<'a>(local_packs: &'a [PackState]) -> Vec<&'a PackState> {
    let mut unique: Vec<&'a PackState> = Vec::new();
    let mut non_unique: Vec<&'a PackState> = Vec::new();
    for p in local_packs {
        let u = p.unique.as_ref().expect("cmp_local_packs sets unique");
        if !u.is_empty() {
            unique.push(p);
        } else {
            non_unique.push(p);
        }
    }

    let mut missing: BTreeSet<ObjectId> = BTreeSet::new();
    for p in local_packs {
        missing.extend(p.remaining.iter().copied());
    }
    for p in &unique {
        missing = sorted_difference(&missing, &p.remaining);
    }

    let mut min: Vec<&'a PackState> = unique;

    if missing.is_empty() {
        return min;
    }

    let mut unique_pack_objects: BTreeSet<ObjectId> = BTreeSet::new();
    for p in local_packs {
        unique_pack_objects.extend(p.remaining.iter().copied());
    }
    unique_pack_objects = sorted_difference(&unique_pack_objects, &missing);

    let mut non_unique_work: Vec<PackStateRef<'a>> = non_unique
        .into_iter()
        .map(|p| {
            let rem = sorted_difference(&p.remaining, &unique_pack_objects);
            PackStateRef {
                inner: p,
                remaining: rem,
            }
        })
        .collect();

    while !non_unique_work.is_empty() {
        sort_pack_list_ref(&mut non_unique_work);
        if non_unique_work[0].remaining.is_empty() {
            break;
        }
        let first = non_unique_work[0].inner;
        min.push(first);
        let first_rem = non_unique_work[0].remaining.clone();
        for item in non_unique_work.iter_mut().skip(1) {
            if !item.remaining.is_empty() {
                item.remaining = sorted_difference(&item.remaining, &first_rem);
            }
        }
        non_unique_work.remove(0);
    }

    min
}

struct PackStateRef<'a> {
    inner: &'a PackState,
    remaining: BTreeSet<ObjectId>,
}

fn sort_pack_list_ref(list: &mut [PackStateRef<'_>]) {
    list.sort_by(|a, b| match b.remaining.len().cmp(&a.remaining.len()) {
        std::cmp::Ordering::Equal => b.inner.all_objects_size.cmp(&a.inner.all_objects_size),
        o => o,
    });
}
