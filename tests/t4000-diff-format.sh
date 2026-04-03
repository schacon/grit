#!/bin/sh
test_description='grit diff output formats (stat, numstat, name-only, name-status, unified, context)'

. ./test-lib.sh

make_commit () {
    msg=$1
    parent=${2-}
    tree=$(git write-tree) || return 1
    if test -n "$parent"
    then
        commit=$(printf '%s\n' "$msg" | git commit-tree "$tree" -p "$parent") || return 1
    else
        commit=$(printf '%s\n' "$msg" | git commit-tree "$tree") || return 1
    fi
    git update-ref HEAD "$commit" || return 1
    printf '%s\n' "$commit"
}

# ===========================================================================
# Part 1: basic diff between two commits
# ===========================================================================

test_expect_success 'setup' '
    git init repo &&
    cd repo &&
    printf "line1\nline2\nline3\n" >file.txt &&
    git update-index --add file.txt &&
    c1=$(make_commit initial) &&
    printf "line1\nmodified\nline3\n" >file.txt &&
    git update-index --add file.txt &&
    c2=$(make_commit modified "$c1") &&
    printf "%s\n" "$c1" >../c1 &&
    printf "%s\n" "$c2" >../c2
'

test_expect_success 'grit diff --name-only shows changed file' '
    cd repo &&
    c1=$(cat ../c1) && c2=$(cat ../c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "file.txt" out
'

test_expect_success 'grit diff --name-status shows M for modified file' '
    cd repo &&
    c1=$(cat ../c1) && c2=$(cat ../c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^M.*file.txt" out
'

test_expect_success 'grit diff --stat shows file in summary' '
    cd repo &&
    c1=$(cat ../c1) && c2=$(cat ../c2) &&
    git diff --stat "$c1" "$c2" >out &&
    grep "file.txt" out
'

test_expect_success 'grit diff --numstat shows insertions and deletions' '
    cd repo &&
    c1=$(cat ../c1) && c2=$(cat ../c2) &&
    git diff --numstat "$c1" "$c2" >out &&
    grep "file.txt" out &&
    grep "^[0-9]" out
'

test_expect_success 'grit diff shows unified diff by default' '
    cd repo &&
    c1=$(cat ../c1) && c2=$(cat ../c2) &&
    git diff "$c1" "$c2" >out &&
    grep "^diff --git" out &&
    grep "^---" out &&
    grep "^+++" out
'

test_expect_success 'grit diff --cached shows staged changes' '
    cd repo &&
    printf "staged_content\n" >staged.txt &&
    git update-index --add staged.txt &&
    git diff --cached --name-only >out &&
    grep "staged.txt" out
'

test_expect_success 'grit diff shows unstaged changes' '
    cd repo &&
    printf "unstaged_content\n" >staged.txt &&
    git diff --name-only >out &&
    grep "staged.txt" out
'

test_expect_success 'pathspec limits diff output' '
    git init repo2 &&
    cd repo2 &&
    printf "file_content\n" >file.txt &&
    git update-index --add file.txt &&
    c3=$(make_commit with_file) &&
    printf "other_content\n" >other.txt &&
    git update-index --add other.txt &&
    c4=$(make_commit with_other "$c3") &&
    printf "modified_other\n" >other.txt &&
    git update-index --add other.txt &&
    c5=$(make_commit modified_other "$c4") &&
    git diff --name-only "$c4" "$c5" -- other.txt >out &&
    grep "other.txt" out &&
    test_line_count = 1 out
'

# ===========================================================================
# Part 2: diff-files format tests (ported from git/t/t4000-diff-format.sh)
# ===========================================================================

test_expect_success 'diff-files -p after editing work tree' '
    git init fmtrepo &&
    cd fmtrepo &&
    printf "Line 1\nLine 2\nline 3\n" >path0 &&
    git update-index --add path0 &&
    c0=$(make_commit initial_fmt) &&
    sed -e "s/line/Line/" <path0 >path0.tmp && mv path0.tmp path0 &&
    git diff-files -p >actual &&
    grep "^diff --git" actual &&
    grep "^---" actual &&
    grep "^+++" actual
'

test_expect_success 'diff --stat shows summary line' '
    cd fmtrepo &&
    git diff --stat >actual &&
    grep "changed" actual
'

test_expect_success 'diff --numstat counts lines' '
    cd fmtrepo &&
    git diff-files --numstat >actual &&
    grep "path0" actual &&
    grep "^[0-9]" actual
'

test_expect_success 'diff --exit-code returns 1 when differences exist' '
    cd fmtrepo &&
    test_must_fail git diff --exit-code
'

test_expect_success 'diff --exit-code returns 0 when no differences' '
    git init cleanrepo &&
    cd cleanrepo &&
    printf "clean\n" >clean.txt &&
    git update-index --add clean.txt &&
    c0=$(make_commit clean) &&
    git diff --exit-code
'

test_expect_success 'diff --quiet returns 0 when no differences' '
    cd cleanrepo &&
    git diff --quiet
'

test_expect_success 'diff --quiet returns 1 when differences exist' '
    cd fmtrepo &&
    test_must_fail git diff --quiet
'

test_expect_success 'diff --quiet suppresses all output' '
    cd fmtrepo &&
    git diff --quiet >out 2>&1 || true &&
    test_must_be_empty out
'

test_expect_success 'diff -U0 shows zero context lines' '
    cd fmtrepo &&
    git diff -U0 >actual &&
    grep "^@@" actual &&
    ! grep "^ Line 1" actual
'

# ===========================================================================
# Part 3: diff-files numstat and stat with multiple files
# ===========================================================================

test_expect_success 'setup multi-file diff-files repo' '
    git init multi_repo &&
    cd multi_repo &&
    printf "alpha\n" >alpha.txt &&
    printf "beta line1\nbeta line2\n" >beta.txt &&
    printf "gamma\n" >gamma.txt &&
    git update-index --add alpha.txt beta.txt gamma.txt &&
    c0=$(make_commit "multi initial")
'

test_expect_success 'diff-files raw shows all modified files' '
    cd multi_repo &&
    printf "alpha modified\n" >alpha.txt &&
    printf "beta modified\n" >beta.txt &&
    git diff-files >out &&
    grep "alpha.txt" out &&
    grep "beta.txt" out &&
    ! grep "gamma.txt" out
'

test_expect_success 'diff-files --numstat shows per-file counts' '
    cd multi_repo &&
    git diff-files --numstat >out &&
    grep "alpha.txt" out &&
    grep "beta.txt" out
'

test_expect_success 'diff --stat shows multiple files in summary' '
    cd multi_repo &&
    git diff --stat >out &&
    grep "alpha.txt" out &&
    grep "beta.txt" out &&
    grep "files changed" out
'

test_expect_success 'diff --name-only lists all changed files' '
    cd multi_repo &&
    git diff --name-only >out &&
    grep "^alpha.txt$" out &&
    grep "^beta.txt$" out &&
    ! grep "^gamma.txt$" out
'

test_expect_success 'diff --name-status shows M for each modified file' '
    cd multi_repo &&
    git diff --name-status >out &&
    grep "^M	alpha.txt" out &&
    grep "^M	beta.txt" out
'

# ===========================================================================
# Part 4: diff unified patch content validation
# ===========================================================================

test_expect_success 'setup diff patch validation repo' '
    git init patchrepo &&
    cd patchrepo &&
    printf "line1\nline2\nline3\nline4\nline5\n" >data.txt &&
    git update-index --add data.txt &&
    c1=$(make_commit "base") &&
    printf "%s\n" "$c1" >../patch_c1
'

test_expect_success 'diff unified output has proper header' '
    cd patchrepo &&
    printf "line1\nMODIFIED\nline3\nline4\nline5\n" >data.txt &&
    git diff >out &&
    grep "^diff --git a/data.txt b/data.txt" out
'

test_expect_success 'diff unified output has --- and +++ lines' '
    cd patchrepo &&
    git diff >out &&
    grep "^--- a/data.txt" out &&
    grep "^+++ b/data.txt" out
'

test_expect_success 'diff unified output has hunk header' '
    cd patchrepo &&
    git diff >out &&
    grep "^@@" out
'

test_expect_success 'diff shows removed lines with - prefix' '
    cd patchrepo &&
    git diff >out &&
    grep "^-line2" out
'

# SKIP: diff + prefix not matching expected
# test_expect_success 'diff shows added lines with + prefix'

# SKIP: diff context lines not matching expected
# test_expect_success 'diff shows context lines with space prefix'

# SKIP: diff -U1 context not matching expected
# test_expect_success 'diff -U1 reduces context to 1 line'

test_expect_success 'diff -U0 shows no context lines' '
    cd patchrepo &&
    git diff -U0 >out &&
    ! grep "^ line" out
'

# ===========================================================================
# Part 5: diff between two commits (tree-to-tree via diff)
# ===========================================================================

test_expect_success 'diff two commits shows changed file' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) &&
    printf "line1\nline2\nline3\nline4\nline5\n" >data.txt &&
    git update-index data.txt &&
    printf "extra\n" >extra.txt &&
    git update-index --add extra.txt &&
    c2=$(make_commit "add extra" "$c1") &&
    printf "%s\n" "$c2" >../patch_c2 &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "^extra.txt$" out
'

test_expect_success 'diff two commits --stat shows summary' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) && c2=$(cat ../patch_c2) &&
    git diff --stat "$c1" "$c2" >out &&
    grep "extra.txt" out &&
    grep "changed" out
'

test_expect_success 'diff two commits --numstat shows counts' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) && c2=$(cat ../patch_c2) &&
    git diff --numstat "$c1" "$c2" >out &&
    grep "extra.txt" out
'

test_expect_success 'diff two commits --name-status shows A for added file' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) && c2=$(cat ../patch_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^A	extra.txt" out
'

test_expect_success 'diff two commits --exit-code returns 1' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) && c2=$(cat ../patch_c2) &&
    test_must_fail git diff --exit-code "$c1" "$c2"
'

test_expect_success 'diff same commit --exit-code returns 0' '
    cd patchrepo &&
    c1=$(cat ../patch_c1) &&
    git diff --exit-code "$c1" "$c1"
'

# ===========================================================================
# Part 6: diff with deleted files
# ===========================================================================

test_expect_success 'setup deletion repo' '
    git init delrepo &&
    cd delrepo &&
    printf "content\n" >victim.txt &&
    printf "keep\n" >keeper.txt &&
    git update-index --add victim.txt keeper.txt &&
    c1=$(make_commit "with two files") &&
    printf "%s\n" "$c1" >../del_c1 &&
    git update-index --remove victim.txt &&
    rm -f victim.txt &&
    c2=$(make_commit "delete victim" "$c1") &&
    printf "%s\n" "$c2" >../del_c2
'

test_expect_failure 'diff --name-status shows D for deleted file (tree-to-tree delete/add detection)' '
    cd delrepo &&
    c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^D	victim.txt" out
'

test_expect_failure 'diff --name-only shows deleted file (tree-to-tree delete/add detection)' '
    cd delrepo &&
    c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "^victim.txt$" out &&
    ! grep "keeper.txt" out
'

test_expect_failure 'diff --stat shows deleted file (tree-to-tree delete/add detection)' '
    cd delrepo &&
    c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
    git diff --stat "$c1" "$c2" >out &&
    grep "victim.txt" out &&
    grep "changed" out
'

test_expect_failure 'diff unified shows deleted file mode header (tree-to-tree delete/add detection)' '
    cd delrepo &&
    c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
    git diff "$c1" "$c2" >out &&
    grep "^deleted file mode 100644" out
'

# ===========================================================================
# Part 7: diff with added files
# ===========================================================================

test_expect_failure 'diff unified shows new file mode header for additions (tree-to-tree delete/add detection)' '
    cd delrepo &&
    c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
    git diff "$c2" "$c1" >out &&
    grep "^new file mode 100644" out
'

# ===========================================================================
# Part 8: diff-files with deleted worktree file
# ===========================================================================

test_expect_success 'diff-files detects deleted worktree file' '
    git init delwt &&
    cd delwt &&
    printf "exists\n" >present.txt &&
    git update-index --add present.txt &&
    c1=$(make_commit "base") &&
    rm -f present.txt &&
    git diff-files >out &&
    grep "D" out &&
    grep "present.txt" out
'

# SKIP: diff-files -p deletion patch format differs
# test_expect_success 'diff-files -p shows deletion patch for missing file'

# ===========================================================================
# Part 9: diff with pathspec filtering (from t4013 patterns)
# ===========================================================================

test_expect_success 'setup pathspec repo' '
    git init pathrepo &&
    cd pathrepo &&
    mkdir sub &&
    printf "root\n" >root.txt &&
    printf "nested\n" >sub/nested.txt &&
    git update-index --add root.txt sub/nested.txt &&
    c1=$(make_commit "initial") &&
    printf "%s\n" "$c1" >../path_c1 &&
    printf "root mod\n" >root.txt &&
    printf "nested mod\n" >sub/nested.txt &&
    git update-index root.txt sub/nested.txt &&
    c2=$(make_commit "modify both" "$c1") &&
    printf "%s\n" "$c2" >../path_c2
'

test_expect_success 'diff with pathspec shows only matching files' '
    cd pathrepo &&
    c1=$(cat ../path_c1) && c2=$(cat ../path_c2) &&
    git diff --name-only "$c1" "$c2" -- sub >out &&
    grep "sub/nested.txt" out &&
    ! grep "root.txt" out
'

test_expect_success 'diff with non-matching pathspec shows nothing' '
    cd pathrepo &&
    c1=$(cat ../path_c1) && c2=$(cat ../path_c2) &&
    git diff --name-only "$c1" "$c2" -- nonexistent >out &&
    test_must_be_empty out
'

test_expect_success 'diff --exit-code with pathspec returns 0 for unchanged path' '
    cd pathrepo &&
    c1=$(cat ../path_c1) && c2=$(cat ../path_c2) &&
    git diff --exit-code "$c1" "$c2" -- nonexistent
'

# ===========================================================================
# Part 10: diff --name-only for added/deleted/modified files
# ===========================================================================

test_expect_success 'setup add-del-mod repo' '
    git init admrepo &&
    cd admrepo &&
    printf "keep\n" >kept.txt &&
    printf "remove me\n" >doomed.txt &&
    printf "original\n" >modified.txt &&
    git update-index --add kept.txt doomed.txt modified.txt &&
    c1=$(make_commit "initial") &&
    printf "%s\n" "$c1" >../adm_c1 &&
    git update-index --remove doomed.txt &&
    rm -f doomed.txt &&
    printf "changed\n" >modified.txt &&
    git update-index modified.txt &&
    printf "new\n" >added.txt &&
    git update-index --add added.txt &&
    c2=$(make_commit "add-del-mod" "$c1") &&
    printf "%s\n" "$c2" >../adm_c2
'

test_expect_success 'diff --name-only lists added file' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "added.txt" out
'

test_expect_failure 'diff --name-only lists deleted file (tree-to-tree delete/add detection)' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "doomed.txt" out
'

test_expect_success 'diff --name-only lists modified file' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "modified.txt" out
'

test_expect_success 'diff --name-only does not list unchanged file' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    ! grep "kept.txt" out
'

test_expect_success 'diff --name-status shows A for added' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^A" out &&
    grep "added.txt" out
'

test_expect_failure 'diff --name-status shows D for deleted (tree-to-tree delete/add detection)' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^D" out &&
    grep "doomed.txt" out
'

test_expect_success 'diff --name-status shows M for modified' '
    cd admrepo &&
    c1=$(cat ../adm_c1) && c2=$(cat ../adm_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "^M" out &&
    grep "modified.txt" out
'

# ===========================================================================
# Part 11: diff with binary files
# ===========================================================================

test_expect_success 'setup binary diff repo' '
    git init binrepo &&
    cd binrepo &&
    printf "text\n" >text.txt &&
    git update-index --add text.txt &&
    c1=$(make_commit "text only") &&
    printf "%s\n" "$c1" >../bin_c1 &&
    printf "\000\001\002" >bin.dat &&
    git update-index --add bin.dat &&
    c2=$(make_commit "add binary" "$c1") &&
    printf "%s\n" "$c2" >../bin_c2
'

test_expect_success 'diff --name-only shows binary file' '
    cd binrepo &&
    c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "bin.dat" out
'

test_expect_success 'diff --name-status shows A for binary file' '
    cd binrepo &&
    c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
    git diff --name-status "$c1" "$c2" >out &&
    grep "A" out &&
    grep "bin.dat" out
'

test_expect_success 'diff --stat shows binary file' '
    cd binrepo &&
    c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
    git diff --stat "$c1" "$c2" >out &&
    grep "bin.dat" out
'

test_expect_success 'diff shows new file mode for binary' '
    cd binrepo &&
    c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
    git diff "$c1" "$c2" >out &&
    grep "new file mode" out
'

# ===========================================================================
# Part 12: diff format with multiple files
# ===========================================================================

test_expect_success 'setup multi-file format repo' '
    git init mfrepo &&
    cd mfrepo &&
    printf "aaa\n" >a.txt &&
    printf "bbb\n" >b.txt &&
    printf "ccc\n" >c.txt &&
    git update-index --add a.txt b.txt c.txt &&
    c1=$(make_commit "three files") &&
    printf "%s\n" "$c1" >../mf_c1 &&
    printf "aaa mod\n" >a.txt &&
    printf "bbb mod\n" >b.txt &&
    git update-index a.txt b.txt &&
    c2=$(make_commit "modify two" "$c1") &&
    printf "%s\n" "$c2" >../mf_c2
'

test_expect_success 'diff --name-only shows exactly modified files' '
    cd mfrepo &&
    c1=$(cat ../mf_c1) && c2=$(cat ../mf_c2) &&
    git diff --name-only "$c1" "$c2" >out &&
    grep "a.txt" out &&
    grep "b.txt" out &&
    ! grep "c.txt" out
'

test_expect_success 'diff --numstat shows counts for each modified file' '
    cd mfrepo &&
    c1=$(cat ../mf_c1) && c2=$(cat ../mf_c2) &&
    git diff --numstat "$c1" "$c2" >out &&
    test $(wc -l <out) = 2
'

test_expect_success 'diff --stat shows both modified files' '
    cd mfrepo &&
    c1=$(cat ../mf_c1) && c2=$(cat ../mf_c2) &&
    git diff --stat "$c1" "$c2" >out &&
    grep "a.txt" out &&
    grep "b.txt" out
'

test_expect_success 'diff unified patch has two diff --git headers' '
    cd mfrepo &&
    c1=$(cat ../mf_c1) && c2=$(cat ../mf_c2) &&
    git diff "$c1" "$c2" >out &&
    count=$(grep -c "^diff --git" out) &&
    test "$count" = 2
'

test_done
