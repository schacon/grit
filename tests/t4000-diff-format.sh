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

test_done
