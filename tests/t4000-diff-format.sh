#!/bin/sh
test_description='grit diff output formats (stat, numstat, name-only, name-status)'

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

# ---------------------------------------------------------------------------
# Additional format tests ported from git/t/t4000-diff-format.sh
# ---------------------------------------------------------------------------

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

test_done
