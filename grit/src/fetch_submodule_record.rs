//! Record ref tips and updated-ref OIDs during `git fetch` for `--recurse-submodules` (Git
//! `ref_tips_before_fetch` / `check_for_new_submodule_commits`).

use grit_lib::objects::ObjectId;
use std::cell::RefCell;

thread_local! {
    static RECORD: RefCell<Option<FetchSubmoduleRecord>> = const { RefCell::new(None) };
}

/// When set, submodule-fetch logic can observe which refs moved during this fetch.
pub struct FetchSubmoduleRecord {
    pub tips_before: Vec<ObjectId>,
    pub tips_after: RefCell<Vec<ObjectId>>,
    pub submodule_commits: RefCell<Vec<ObjectId>>,
}

/// Begin recording for this `fetch` process (each nested `grit fetch` subprocess gets its own record).
pub fn begin_fetch_submodule_record(git_dir: &std::path::Path) {
    let tips: Vec<ObjectId> = grit_lib::refs::list_refs(git_dir, "refs/")
        .ok()
        .map(|v| v.into_iter().map(|(_, o)| o).collect())
        .unwrap_or_default();
    RECORD.with(|r| {
        if r.borrow().is_some() {
            return;
        }
        *r.borrow_mut() = Some(FetchSubmoduleRecord {
            tips_before: tips,
            tips_after: RefCell::new(Vec::new()),
            submodule_commits: RefCell::new(Vec::new()),
        });
    });
}

/// Merge ref tips after this fetch into the running record (`git fetch --all` runs multiple fetches).
pub fn finish_record_tips_after(git_dir: &std::path::Path) {
    let tips: Vec<ObjectId> = grit_lib::refs::list_refs(git_dir, "refs/")
        .ok()
        .map(|v| v.into_iter().map(|(_, o)| o).collect())
        .unwrap_or_default();
    RECORD.with(|r| {
        if let Some(rec) = r.borrow_mut().as_mut() {
            let mut after = rec.tips_after.borrow_mut();
            for o in tips {
                if !after.contains(&o) {
                    after.push(o);
                }
            }
        }
    });
}

/// Record a superproject commit OID whose tree may contain new submodule pointers (Git
/// `check_for_new_submodule_commits`).
pub fn record_submodule_tip(oid: &ObjectId) {
    RECORD.with(|r| {
        if let Some(rec) = r.borrow().as_ref() {
            rec.submodule_commits.borrow_mut().push(*oid);
        }
    });
}

/// Take recorded state for `fetch_submodules` (clears thread-local).
pub fn take_fetch_submodule_record() -> Option<FetchSubmoduleRecord> {
    RECORD.with(|r| r.borrow_mut().take())
}
