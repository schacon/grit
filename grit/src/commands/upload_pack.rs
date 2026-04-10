//! `grit upload-pack` — send objects for fetch (server side).
//!
//! Invoked on the remote side of a fetch. Advertises refs in pkt-line format,
//! negotiates want/have (protocol v0, `multi_ack_detailed`), then streams a
//! packfile (side-band-64k) to the client.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::diff::zero_oid;
use grit_lib::hide_refs;
use grit_lib::merge_base;
use grit_lib::objects::{ObjectId, ObjectKind};
use grit_lib::ref_namespace;
use grit_lib::refs;
use grit_lib::repo::Repository;
use grit_lib::rev_list::{
    shallow_boundary_oids, shallow_grafts_for_upload_pack_deepen,
    shallow_grafts_for_upload_pack_rev_list,
};
use grit_lib::rev_parse;
use grit_lib::state::resolve_head;
use grit_lib::state::HeadState;
use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::commands::serve_v2::{serve_loop, ServerCaps};
use crate::pkt_line;
use crate::protocol_wire;
use crate::trace2_transfer;

/// Arguments for `grit upload-pack`.
#[derive(Debug, ClapArgs)]
#[command(about = "Send objects for fetch (server side)")]
pub struct Args {
    /// Path to the repository (bare or non-bare).
    #[arg(value_name = "DIRECTORY")]
    pub directory: PathBuf,

    /// Only advertise refs and capabilities, then exit.
    #[arg(long)]
    pub advertise_refs: bool,
}

fn next_upload_pack_packet(
    stdin: &mut impl io::Read,
    pending: &mut VecDeque<pkt_line::Packet>,
) -> io::Result<Option<pkt_line::Packet>> {
    if let Some(p) = pending.pop_front() {
        return Ok(Some(p));
    }
    pkt_line::read_packet(stdin)
}

pub fn run(args: Args) -> Result<()> {
    // Match `git upload-pack`: default `GIT_NO_LAZY_FETCH=1` so remote `pack-objects` does not
    // lazy-fetch missing blobs (t0411-clone-from-partial, promisor clone via upload-pack).
    if std::env::var("GIT_NO_LAZY_FETCH")
        .ok()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        std::env::set_var("GIT_NO_LAZY_FETCH", "1");
    }

    let repo = open_repo(&args.directory).with_context(|| {
        format!(
            "could not open repository at '{}'",
            args.directory.display()
        )
    })?;
    repo.enforce_safe_directory_git_dir()?;

    trace2_transfer::emit_negotiated_version_from_git_protocol_env();

    let server_proto = protocol_wire::server_protocol_version_from_git_protocol_env();
    if server_proto == 2 {
        let caps = ServerCaps::load(&repo.git_dir);
        if args.advertise_refs {
            let mut out = io::stdout();
            caps.advertise(&mut out)?;
            out.flush()?;
            return Ok(());
        }
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let stdout = io::stdout();
        let mut out = stdout.lock();
        caps.advertise(&mut out)?;
        out.flush()?;
        drop(out);
        return serve_loop(&mut input, &repo.git_dir, &caps);
    }

    if args.advertise_refs {
        return advertise_refs_with_caps(&repo, server_proto);
    }

    let mut out = io::stdout();
    if server_proto == 1 {
        pkt_line::write_line(&mut out, "version 1")?;
        out.flush()?;
    }
    write_ref_advertisement(&mut out, &repo.git_dir)?;
    pkt_line::write_flush(&mut out)?;
    out.flush()?;

    let mut stdin = io::stdin();
    let mut pending: VecDeque<pkt_line::Packet> = VecDeque::new();
    let mut wants: Vec<ObjectId> = Vec::new();
    let mut multi_ack_detailed = false;
    let mut no_done = false;
    let mut parsed_first_want_caps = false;
    let mut client_shallow: Vec<ObjectId> = Vec::new();
    let mut deepen: Option<usize> = None;
    let mut deepen_since: Option<i64> = None;
    let mut deepen_not: Vec<ObjectId> = Vec::new();
    let mut use_thin_pack_cli = false;
    let mut list_objects_filter: Option<String> = None;

    loop {
        match next_upload_pack_packet(&mut stdin, &mut pending)? {
            None => break,
            Some(pkt_line::Packet::Flush) => {
                // Stateless HTTP may send two flush packets after the want list; consume extras so
                // the first negotiation round is not mistaken for stray input (t5539).
                loop {
                    match pkt_line::read_packet(&mut stdin)? {
                        None => break,
                        Some(pkt_line::Packet::Flush) => continue,
                        Some(other) => {
                            pending.push_back(other);
                            break;
                        }
                    }
                }
                break;
            }
            Some(pkt_line::Packet::Data(line)) => {
                if let Some(spec) = line.strip_prefix("filter ") {
                    if list_objects_filter.is_some() {
                        bail!("duplicate filter line from fetch client");
                    }
                    list_objects_filter = Some(spec.trim().to_owned());
                    continue;
                }
                // One pkt-line payload may concatenate many commands without `\n` (stateless HTTP).
                let mut pos = 0usize;
                while pos < line.len() {
                    let Some(idx) = line[pos..].find("want ") else {
                        break;
                    };
                    let start = pos + idx;
                    let after_want = &line[start + "want ".len()..];
                    let hex = after_want.split_whitespace().next().unwrap_or("");
                    if hex.len() != 40 {
                        pos = start + "want ".len();
                        continue;
                    }
                    let after_oid = &after_want[hex.len()..];
                    let next_want = after_oid.find("want ").unwrap_or(after_oid.len());
                    let segment = &after_oid[..next_want];
                    let features = segment.trim();
                    if !parsed_first_want_caps {
                        parsed_first_want_caps = true;
                        if features.contains("multi_ack_detailed") {
                            multi_ack_detailed = true;
                        }
                        if features.contains("no-done") {
                            no_done = true;
                        }
                    }
                    if features.contains("thin-pack") {
                        use_thin_pack_cli = true;
                    }
                    if wants.is_empty() {
                        if let Some(sid) = trace2_transfer::extract_session_id_feature(features) {
                            trace2_transfer::emit_client_sid(sid);
                        }
                    }
                    if let Ok(oid) = ObjectId::from_hex(hex) {
                        wants.push(oid);
                    }
                    pos = start + "want ".len() + hex.len() + next_want;
                }
                scan_shallow_deepen_in_payload(
                    &line,
                    &repo,
                    &mut client_shallow,
                    &mut deepen,
                    &mut deepen_since,
                    &mut deepen_not,
                );
            }
            _ => {}
        }
    }
    if wants.is_empty() {
        return Ok(());
    }

    // Fetch clients may send the same `want` OID twice (e.g. duplicate pkt-lines). `pack-objects
    // --revs` treats each positive rev line as a separate walk root; duplicates corrupt the pack.
    let mut want_unique: Vec<ObjectId> = Vec::new();
    let mut want_seen: HashSet<ObjectId> = HashSet::new();
    for w in wants {
        if want_seen.insert(w) {
            want_unique.push(w);
        }
    }

    let want_set: HashSet<ObjectId> = want_unique.iter().copied().collect();

    let server_shallow: Vec<ObjectId> = shallow_boundary_oids(&repo.git_dir).into_iter().collect();
    let mut pack_shallow_grafts: Vec<ObjectId> = Vec::new();
    if let Some(d) = deepen {
        pack_shallow_grafts =
            shallow_grafts_for_upload_pack_deepen(&repo, &want_unique, &client_shallow, d);
        for oid in &pack_shallow_grafts {
            pkt_line::write_line(&mut out, &format!("shallow {}", oid.to_hex()))?;
        }
        pkt_line::write_flush(&mut out)?;
        out.flush()?;
    } else if deepen_since.is_some() || !deepen_not.is_empty() {
        pack_shallow_grafts = shallow_grafts_for_upload_pack_rev_list(
            &repo,
            &want_unique,
            &client_shallow,
            deepen_since,
            &deepen_not,
        )?;
        for oid in &pack_shallow_grafts {
            pkt_line::write_line(&mut out, &format!("shallow {}", oid.to_hex()))?;
        }
        pkt_line::write_flush(&mut out)?;
        out.flush()?;
    }

    let mut stdin_shallow: Vec<ObjectId> = server_shallow;
    for oid in &pack_shallow_grafts {
        if !stdin_shallow.contains(oid) {
            stdin_shallow.push(*oid);
        }
    }
    let shallow_pack = !stdin_shallow.is_empty();

    let mut got_common = false;
    let mut got_other = false;
    let mut last_hex = String::new();
    let mut client_known: HashSet<ObjectId> = HashSet::new();
    let mut client_have_commits: Vec<ObjectId> = Vec::new();
    // After we respond to `deepen`, fetch-pack replays the full initial want/shallow/deepen
    // pkt-lines (stateless RPC) before the first real `have` round. Skip until negotiation starts.
    let mut skip_until_have = deepen.is_some() || deepen_since.is_some() || !deepen_not.is_empty();
    let mut sent_ready = false;
    let mut ended_with_no_done_ack = false;

    'negotiation: loop {
        match next_upload_pack_packet(&mut stdin, &mut pending)? {
            None => break,
            Some(pkt_line::Packet::Flush) => {
                if multi_ack_detailed
                    && got_common
                    && !got_other
                    && ok_to_give_up(&repo, &want_set, &client_known)
                {
                    pkt_line::write_line(&mut out, &format!("ACK {last_hex} ready"))?;
                    sent_ready = true;
                }
                if got_common || multi_ack_detailed {
                    pkt_line::write_line(&mut out, "NAK")?;
                }
                got_common = false;
                got_other = false;
                out.flush()?;
                if no_done && sent_ready && !last_hex.is_empty() {
                    pkt_line::write_line(&mut out, &format!("ACK {last_hex}"))?;
                    out.flush()?;
                    ended_with_no_done_ack = true;
                    break 'negotiation;
                }
            }
            Some(pkt_line::Packet::Data(line)) => {
                let mut saw_done = false;
                let mut pos = 0usize;
                let bytes = line.as_bytes();
                while pos < bytes.len() {
                    if skip_until_have {
                        if bytes[pos..].starts_with(b"have ") {
                            skip_until_have = false;
                        } else if pos + 4 <= bytes.len()
                            && &bytes[pos..pos + 4] == b"done"
                            && (pos + 4 == bytes.len() || bytes[pos + 4] <= b' ')
                        {
                            skip_until_have = false;
                        } else if bytes[pos..].starts_with(b"want ") {
                            pos = skip_past_want_segment(&line, pos);
                            continue;
                        } else if bytes[pos..].starts_with(b"shallow ") {
                            pos = skip_past_shallow_oid(&line, pos);
                            continue;
                        } else if bytes[pos..].starts_with(b"deepen-since ") {
                            pos = skip_to_next_negotiation_cmd(&line, pos + b"deepen-since ".len());
                            continue;
                        } else if bytes[pos..].starts_with(b"deepen-not ") {
                            pos = skip_to_next_negotiation_cmd(&line, pos + b"deepen-not ".len());
                            continue;
                        } else if bytes[pos..].starts_with(b"deepen ") {
                            pos = skip_to_next_negotiation_cmd(&line, pos + b"deepen ".len());
                            continue;
                        } else if bytes[pos..].starts_with(b"filter ") {
                            pos = skip_to_next_negotiation_cmd(&line, pos + b"filter ".len());
                            continue;
                        } else {
                            skip_until_have = false;
                        }
                    }

                    if pos + 4 <= bytes.len()
                        && &bytes[pos..pos + 4] == b"done"
                        && (pos + 4 == bytes.len() || bytes[pos + 4] <= b' ')
                    {
                        if !last_hex.is_empty() && multi_ack_detailed {
                            pkt_line::write_line(&mut out, &format!("ACK {last_hex}"))?;
                        } else if got_common {
                            pkt_line::write_line(&mut out, &format!("ACK {last_hex}"))?;
                        } else {
                            pkt_line::write_line(&mut out, "NAK")?;
                        }
                        out.flush()?;
                        saw_done = true;
                        break;
                    }

                    if bytes[pos..].starts_with(b"have ") {
                        let hstart = pos + b"have ".len();
                        let rest = &line[hstart..];
                        let n = rest
                            .find(|c: char| !c.is_ascii_hexdigit())
                            .unwrap_or(rest.len());
                        if n == 40 {
                            if let Ok(oid) = ObjectId::from_hex(&rest[..40]) {
                                if repo.odb.read(&oid).is_err() {
                                    got_other = true;
                                    if multi_ack_detailed
                                        && ok_to_give_up(&repo, &want_set, &client_known)
                                    {
                                        let hx = oid.to_hex();
                                        pkt_line::write_line(&mut out, &format!("ACK {hx} ready"))?;
                                        last_hex = hx;
                                        sent_ready = true;
                                    }
                                } else {
                                    got_common = true;
                                    last_hex = oid.to_hex();
                                    client_have_commits.push(oid);
                                    merge_ancestors_into(&repo, oid, &mut client_known)?;
                                    if multi_ack_detailed {
                                        pkt_line::write_line(
                                            &mut out,
                                            &format!("ACK {last_hex} common"),
                                        )?;
                                    } else {
                                        pkt_line::write_line(&mut out, &format!("ACK {last_hex}"))?;
                                    }
                                }
                            }
                            out.flush()?;
                            pos = hstart + 40;
                            continue;
                        }
                    }

                    pos += 1;
                }
                if saw_done {
                    break 'negotiation;
                }
            }
            _ => {}
        }
    }

    if deepen.is_some()
        || deepen_since.is_some()
        || !deepen_not.is_empty()
        || ended_with_no_done_ack
    {
        match next_upload_pack_packet(&mut stdin, &mut pending)? {
            None => {}
            Some(pkt_line::Packet::Flush) => {}
            Some(other) => pending.push_back(other),
        }
        loop {
            match next_upload_pack_packet(&mut stdin, &mut pending)? {
                None => break,
                Some(pkt_line::Packet::Flush) => break,
                Some(pkt_line::Packet::Data(_))
                | Some(pkt_line::Packet::Delim)
                | Some(pkt_line::Packet::ResponseEnd) => {}
            }
        }
    }

    // Only short-circuit to an empty pack when every `want` is a commit the client already has.
    // `client_known` includes blob OIDs reachable from `have` commits (server-side walk), but a
    // partial-clone client may still lack those blobs — never treat a blob/tree `want` as
    // satisfied by that set (t0410 lazy fetch).
    let already_have_all = wants_include_only_commits(&repo, &want_unique)
        && want_unique.iter().all(|w| client_known.contains(w));
    if already_have_all {
        let pack = crate::pack_objects_upload::empty_packfile_v2_bytes();
        crate::pack_objects_upload::write_sideband_64k(&mut out, &pack)?;
    } else {
        // Thin packs subtract the full closure of `have` commits. That is only safe when every
        // `want` is a commit OID; blob/tree lazy-fetch wants must use a self-contained pack
        // (t0410 partial-clone explicit wants).
        let thin = use_thin_pack_cli
            && !client_have_commits.is_empty()
            && wants_include_only_commits(&repo, &want_unique);
        let mut child = crate::pack_objects_upload::spawn_pack_objects_upload(
            &repo.git_dir,
            thin,
            shallow_pack,
            list_objects_filter.as_deref(),
        )?;
        {
            let mut pin = child.stdin.take().context("pack-objects stdin")?;
            crate::pack_objects_upload::write_pack_objects_revs_stdin(
                &mut pin,
                &want_unique,
                &client_have_commits,
                &stdin_shallow,
                shallow_pack,
            )?;
        }
        crate::pack_objects_upload::drain_pack_objects_child(child, &mut out, true)?;
    }

    pkt_line::write_flush(&mut out)?;
    out.flush()?;
    Ok(())
}

/// Returns `true` when every wanted OID resolves to a commit object in the server ODB.
fn wants_include_only_commits(repo: &Repository, wants: &[ObjectId]) -> bool {
    for w in wants {
        let Ok(obj) = repo.odb.read(w) else {
            return false;
        };
        if obj.kind != ObjectKind::Commit {
            return false;
        }
    }
    true
}

fn negotiation_cmd_start(line: &str, from: usize) -> Option<usize> {
    let s = &line[from..];
    const NEEDLES: &[&str] = &[
        "want ",
        "have ",
        "shallow ",
        "deepen-since ",
        "deepen-not ",
        "deepen ",
        "filter ",
        "done",
    ];
    let mut best: Option<usize> = None;
    for n in NEEDLES {
        if let Some(i) = s.find(n) {
            let abs = from + i;
            best = Some(best.map(|b| b.min(abs)).unwrap_or(abs));
        }
    }
    best
}

fn skip_to_next_negotiation_cmd(line: &str, pos: usize) -> usize {
    negotiation_cmd_start(line, pos).unwrap_or(line.len())
}

fn skip_past_want_segment(line: &str, pos: usize) -> usize {
    let start = pos;
    if !line[pos..].starts_with("want ") {
        return (start + 1).min(line.len());
    }
    let after = pos + "want ".len();
    let rest = &line[after..];
    let n = rest
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(rest.len());
    if n != 40 {
        return (start + 1).min(line.len());
    }
    let after_oid = after + 40;
    line[after_oid..]
        .find("want ")
        .map(|i| after_oid + i)
        .unwrap_or(line.len())
}

fn skip_past_shallow_oid(line: &str, pos: usize) -> usize {
    if !line[pos..].starts_with("shallow ") {
        return (pos + 1).min(line.len());
    }
    let after = pos + "shallow ".len();
    let rest = &line[after..];
    let n = rest
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(rest.len());
    if n == 40 {
        after + 40
    } else {
        (pos + 1).min(line.len())
    }
}

fn scan_shallow_deepen_in_payload(
    line: &str,
    repo: &Repository,
    client_shallow: &mut Vec<ObjectId>,
    deepen: &mut Option<usize>,
    deepen_since: &mut Option<i64>,
    deepen_not: &mut Vec<ObjectId>,
) {
    for sub in line.split('\n') {
        let sub = sub.trim();
        if sub.is_empty() {
            continue;
        }
        if let Some(hex) = sub.strip_prefix("shallow ") {
            let hex = hex.split_whitespace().next().unwrap_or("");
            if hex.len() == 40 {
                if let Ok(oid) = ObjectId::from_hex(hex) {
                    if !client_shallow.contains(&oid) {
                        client_shallow.push(oid);
                    }
                }
            }
            continue;
        }
        if let Some(rest) = sub.strip_prefix("deepen ") {
            let arg = rest.split_whitespace().next().unwrap_or("");
            if let Ok(d) = arg.parse::<usize>() {
                if d > 0 {
                    *deepen = Some(d);
                    *deepen_since = None;
                    deepen_not.clear();
                }
            }
            continue;
        }
        if let Some(rest) = sub.strip_prefix("deepen-since ") {
            let arg = rest.split_whitespace().next().unwrap_or("");
            if let Ok(ts) = arg.parse::<i64>() {
                if ts > 0 {
                    *deepen_since = Some(ts);
                    *deepen = None;
                }
            }
            continue;
        }
        if let Some(rest) = sub.strip_prefix("deepen-not ") {
            let refname = rest.split_whitespace().next().unwrap_or("");
            if refname.is_empty() {
                continue;
            }
            *deepen = None;
            if let Ok(oid) = rev_parse::resolve_revision_without_index_dwim(repo, refname) {
                if !deepen_not.contains(&oid) {
                    deepen_not.push(oid);
                }
            }
            continue;
        }
    }
}

fn merge_ancestors_into(
    repo: &Repository,
    tip: ObjectId,
    into: &mut HashSet<ObjectId>,
) -> Result<()> {
    let boundaries = shallow_boundary_oids(&repo.git_dir);
    // Best-effort: negotiation must not abort if a `have` points at a commit whose parent chain
    // hits a missing object (server shallow edge, replace ref, or corrupt tip). Git skips bad
    // links rather than killing upload-pack before ACK lines (t5539).
    if let Ok(anc) = ancestor_closure_respecting_shallow(repo, tip, &boundaries) {
        into.extend(anc);
    }
    Ok(())
}

fn ancestor_closure_respecting_shallow(
    repo: &Repository,
    tip: ObjectId,
    shallow_boundaries: &HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>> {
    use grit_lib::objects::{parse_commit, ObjectKind};

    let mut visited = HashSet::new();
    let mut q = VecDeque::new();
    q.push_back(tip);
    while let Some(oid) = q.pop_front() {
        if !visited.insert(oid) {
            continue;
        }
        if shallow_boundaries.contains(&oid) {
            continue;
        }
        let obj = repo.odb.read(&oid).map_err(|e| anyhow::anyhow!("{e}"))?;
        if obj.kind != ObjectKind::Commit {
            continue;
        }
        let commit = parse_commit(&obj.data).map_err(|e| anyhow::anyhow!("{e}"))?;
        for p in commit.parents {
            q.push_back(p);
        }
    }
    Ok(visited)
}

fn ok_to_give_up(
    repo: &Repository,
    wants: &HashSet<ObjectId>,
    client_known: &HashSet<ObjectId>,
) -> bool {
    if client_known.is_empty() {
        return false;
    }
    for w in wants {
        let mut covered = false;
        for &h in client_known {
            if h == *w {
                covered = true;
                break;
            }
            if merge_base::is_ancestor(repo, h, *w).unwrap_or(false) {
                covered = true;
                break;
            }
        }
        if !covered {
            return false;
        }
    }
    true
}

fn write_ref_advertisement(w: &mut impl Write, git_dir: &Path) -> Result<()> {
    let version = crate::version_string();
    let set = ConfigSet::load(Some(git_dir), false).unwrap_or_default();
    let object_format = set
        .get("extensions.objectformat")
        .or_else(|| set.get("extensions.objectFormat"))
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "sha1".to_owned());
    let hide = hide_refs::hide_ref_patterns_uploadpack(&set);
    let head_sym = refs::read_symbolic_ref(git_dir, "HEAD")
        .ok()
        .flatten()
        .unwrap_or_else(|| "refs/heads/main".to_owned());
    let head_sym_caps = ref_namespace::strip_namespace_prefix(&head_sym).into_owned();
    let mut caps = format!(
        "multi_ack thin-pack side-band side-band-64k ofs-delta shallow deepen-since deepen-not \
         deepen-relative no-progress include-tag multi_ack_detailed allow-tip-sha1-in-want \
         allow-reachable-sha1-in-want no-done symref=HEAD:{} filter object-format={object_format} \
         agent=git/{} ref-in-want",
        head_sym_caps, version,
    );
    if trace2_transfer::transfer_advertise_sid_enabled(git_dir) {
        let sid = trace2_transfer::trace2_session_id_wire_once();
        caps.push_str(" session-id=");
        caps.push_str(&sid);
    }

    let mut first = true;
    let ns_active = ref_namespace::ref_storage_prefix().is_some();
    if let Ok(head_oid) = refs::resolve_ref(git_dir, "HEAD") {
        let full_head = ref_namespace::storage_ref_name("HEAD");
        if hide_refs::ref_is_hidden("HEAD", &full_head, &hide) {
            first = true;
        } else if ns_active {
            if let Ok(Some(target)) = refs::read_symbolic_ref(git_dir, "HEAD") {
                let display_target = ref_namespace::strip_namespace_prefix(&target);
                let sym_caps = caps.replace(
                    &format!("symref=HEAD:{head_sym_caps}"),
                    &format!("symref=HEAD:{display_target}"),
                );
                let line = format!("{}\tHEAD\0{sym_caps}\n", head_oid.to_hex());
                let len = 4 + line.len();
                write!(w, "{:04x}{}", len, line)?;
                first = false;
            } else {
                let line = format!("{}\tHEAD\0{caps}\n", head_oid.to_hex());
                let len = 4 + line.len();
                write!(w, "{:04x}{}", len, line)?;
                first = false;
            }
        } else {
            let line = format!("{}\tHEAD\0{caps}\n", head_oid.to_hex());
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        }
    } else {
        // Unborn or dangling `HEAD` symref: Git omits a `HEAD` advertisement and may use the
        // first non-branch/non-tag ref as the capability carrier (see `t5700` branchless remote).
        let under_refs = refs::list_refs(git_dir, "refs/")?;
        let non_standard: Vec<(String, ObjectId)> = under_refs
            .into_iter()
            .filter(|(n, _)| !n.starts_with("refs/heads/") && !n.starts_with("refs/tags/"))
            .collect();
        if !non_standard.is_empty() {
            let mut cap_next = first;
            let mut wrote_any = false;
            for (refname, oid) in non_standard {
                let full = ref_namespace::storage_ref_name(&refname);
                if hide_refs::ref_is_hidden(&refname, &full, &hide) {
                    continue;
                }
                let display = ref_namespace::strip_namespace_prefix(&refname);
                let line = if cap_next {
                    cap_next = false;
                    format!("{}\t{}\0{}\n", oid.to_hex(), display, caps)
                } else {
                    format!("{}\t{}\n", oid.to_hex(), display)
                };
                let len = 4 + line.len();
                write!(w, "{:04x}{}", len, line)?;
                wrote_any = true;
            }
            if wrote_any {
                first = false;
            }
        } else if let Ok(HeadState::Detached { oid }) = resolve_head(git_dir) {
            let line = format!("{}\tHEAD\0{}\n", oid.to_hex(), caps);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        } else if let Ok(HeadState::Branch { oid: Some(oid), .. }) = resolve_head(git_dir) {
            let line = format!("{}\tHEAD\0{}\n", oid.to_hex(), caps);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        } else if let Ok(HeadState::Branch { oid: None, .. }) = resolve_head(git_dir) {
            let z = zero_oid();
            let line = format!("{}\tHEAD\0{}\n", z.to_hex(), caps);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        }
    }

    let all_refs = list_all_refs(git_dir)?;
    for (refname, oid) in &all_refs {
        let full = ref_namespace::storage_ref_name(refname);
        if hide_refs::ref_is_hidden(refname, &full, &hide) {
            continue;
        }
        let display = ref_namespace::strip_namespace_prefix(refname);
        if first {
            let line = format!("{}\t{}\0{}\n", oid.to_hex(), display, caps);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
            first = false;
        } else {
            let line = format!("{}\t{}\n", oid.to_hex(), display);
            let len = 4 + line.len();
            write!(w, "{:04x}{}", len, line)?;
        }
    }

    Ok(())
}

fn advertise_refs_with_caps(repo: &Repository, server_proto: u8) -> Result<()> {
    let mut out = io::stdout();
    if server_proto == 1 {
        pkt_line::write_line(&mut out, "version 1")?;
        out.flush()?;
    }
    write_ref_advertisement(&mut out, &repo.git_dir)?;
    write!(out, "0000")?;
    out.flush()?;
    Ok(())
}

fn list_all_refs(git_dir: &Path) -> Result<Vec<(String, ObjectId)>> {
    let mut result = Vec::new();
    let mut prefixes = vec![
        "refs/heads/",
        "refs/tags/",
        "refs/remotes/",
        "refs/notes/",
    ];
    if ref_namespace::ref_storage_prefix().is_none() {
        prefixes.push("refs/namespaces/");
    }
    for prefix in prefixes {
        if let Ok(entries) = refs::list_refs(git_dir, prefix) {
            result.extend(entries);
        }
    }
    Ok(result)
}

/// Open a repository (bare or non-bare).
fn open_repo(path: &Path) -> Result<Repository> {
    if path.is_file() {
        let work_tree = path.parent().map(std::path::Path::to_path_buf);
        let git_dir = grit_lib::repo::resolve_dot_git(path)?;
        return Repository::open(&git_dir, work_tree.as_deref()).map_err(Into::into);
    }
    if let Ok(repo) = Repository::open(path, None) {
        return Ok(repo);
    }
    let dot_git = path.join(".git");
    if dot_git.is_file() {
        let resolved = grit_lib::repo::resolve_dot_git(&dot_git)?;
        return Repository::open(&resolved, Some(path)).map_err(Into::into);
    }
    Repository::open(&dot_git, Some(path)).map_err(Into::into)
}
