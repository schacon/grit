#!/bin/sh
# Ported from git/t/t4011 patterns — tests for 'grit diff-tree'.

test_description='grit diff-tree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

make_commit () {
	msg=$1
	parent=${2-}
	tree=$(git write-tree) || return 1
	if test -n "$parent"; then
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree" -p "$parent") || return 1
	else
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree") || return 1
	fi
	git update-ref HEAD "$commit" || return 1
	printf '%s\n' "$commit"
}

# ---------------------------------------------------------------------------
# Setup — all state files written to the trash root for easy cross-test access.
# ---------------------------------------------------------------------------

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	printf "hello\n" >file.txt &&
	git update-index --add file.txt &&
	commit1=$(make_commit "initial") &&
	test -n "$commit1" &&
	printf "%s\n" "$commit1" >../commit1 &&
	tree1=$(git cat-file -p "$commit1" | grep "^tree" | awk "{print \$2}") &&
	printf "%s\n" "$tree1" >../tree1
'

test_expect_success 'setup second commit' '
	cd repo &&
	printf "world\n" >>file.txt &&
	git update-index --add file.txt &&
	c1=$(cat ../commit1) &&
	commit2=$(make_commit "second" "$c1") &&
	test -n "$commit2" &&
	printf "%s\n" "$commit2" >../commit2 &&
	tree2=$(git cat-file -p "$commit2" | grep "^tree" | awk "{print \$2}") &&
	printf "%s\n" "$tree2" >../tree2
'

# ---------------------------------------------------------------------------
# Two-tree mode
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree two trees produces raw output' '
	cd repo &&
	t1=$(cat ../tree1) &&
	t2=$(cat ../tree2) &&
	git diff-tree "$t1" "$t2" >out &&
	grep "M	file.txt" out
'

test_expect_success 'diff-tree two trees raw line starts with colon' '
	cd repo &&
	t1=$(cat ../tree1) &&
	t2=$(cat ../tree2) &&
	git diff-tree "$t1" "$t2" >out &&
	grep "^:" out
'

# ---------------------------------------------------------------------------
# Single-commit mode
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree single commit shows changes vs parent' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree "$c2" >out &&
	grep "M	file.txt" out
'

test_expect_success 'diff-tree single commit raw output has correct status' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree "$c2" >out &&
	grep "^:100644 100644 " out
'

test_expect_success 'diff-tree root commit without --root produces no output' '
	cd repo &&
	c1=$(cat ../commit1) &&
	git diff-tree "$c1" >out &&
	test_must_be_empty out
'

test_expect_success 'diff-tree root commit with --root shows files' '
	cd repo &&
	c1=$(cat ../commit1) &&
	git diff-tree --root "$c1" >out &&
	grep "A	file.txt" out
'

# ---------------------------------------------------------------------------
# Recursive flag
# ---------------------------------------------------------------------------

test_expect_success 'setup nested directory' '
	cd repo &&
	mkdir -p sub &&
	printf "nested\n" >sub/nested.txt &&
	git update-index --add sub/nested.txt &&
	c2=$(cat ../commit2) &&
	commit3=$(make_commit "add nested" "$c2") &&
	printf "%s\n" "$commit3" >../commit3 &&
	tree3=$(git cat-file -p "$commit3" | grep "^tree" | awk "{print \$2}") &&
	printf "%s\n" "$tree3" >../tree3
'

test_expect_success 'diff-tree -r recurses into subdirs' '
	cd repo &&
	c3=$(cat ../commit3) &&
	git diff-tree -r "$c3" >out &&
	grep "sub/nested.txt" out
'

test_expect_success 'diff-tree without -r does not recurse into subdirs' '
	cd repo &&
	t2=$(cat ../tree2) &&
	t3=$(cat ../tree3) &&
	git diff-tree "$t2" "$t3" >out &&
	! grep "sub/nested.txt" out
'

# ---------------------------------------------------------------------------
# Output formats
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree -p produces patch output' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r -p "$c2" >out &&
	grep "^diff --git" out &&
	grep "^+world" out
'

test_expect_success 'diff-tree --patch produces patch output' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r --patch "$c2" >out &&
	grep "^diff --git" out
'

test_expect_success 'diff-tree --name-only shows only file names' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r --name-only "$c2" >out &&
	grep "^file.txt$" out &&
	! grep "^:" out
'

test_expect_success 'diff-tree --name-status shows status letter and name' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r --name-status "$c2" >out &&
	grep "^M	file.txt" out
'

test_expect_success 'diff-tree --stat shows diffstat' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r --stat "$c2" >out &&
	grep "file.txt" out &&
	grep "changed" out
'

# ---------------------------------------------------------------------------
# --stdin mode
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree --stdin reads commit OID and shows diff' '
	cd repo &&
	c2=$(cat ../commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin >out &&
	grep "M	file.txt" out
'

test_expect_success 'diff-tree --stdin prints commit-id header' '
	cd repo &&
	c2=$(cat ../commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin >out &&
	head -1 out >first_line &&
	grep "^[0-9a-f]\{40\}$" first_line
'

test_expect_success 'diff-tree --stdin --no-commit-id suppresses header' '
	cd repo &&
	c2=$(cat ../commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin --no-commit-id >out &&
	grep "^:" out &&
	! head -1 out | grep "^[0-9a-f]\{40\}$"
'

test_expect_success 'diff-tree --stdin with two tree OIDs compares them' '
	cd repo &&
	t1=$(cat ../tree1) &&
	t2=$(cat ../tree2) &&
	printf "%s %s\n" "$t1" "$t2" | git diff-tree --stdin >out &&
	head -1 out >first_line &&
	grep "$t1" first_line &&
	grep "$t2" first_line &&
	grep "M	file.txt" out
'

test_expect_success 'diff-tree --stdin passes through non-OID lines' '
	cd repo &&
	printf "not-a-sha1\n" | git diff-tree --stdin >out &&
	grep "not-a-sha1" out
'

# ---------------------------------------------------------------------------
# Path-limiting
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree with pathspec limits output' '
	cd repo &&
	c3=$(cat ../commit3) &&
	git diff-tree -r "$c3" -- sub >out &&
	grep "sub/nested.txt" out
'

test_expect_success 'diff-tree with pathspec excludes non-matching files' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r "$c2" -- nonexistent.txt >out &&
	test_must_be_empty out
'

# ---------------------------------------------------------------------------
# Additional tests ported from git/t/t4011-diff-tree.sh patterns
# ---------------------------------------------------------------------------

test_expect_success 'diff-tree --no-commit-id suppresses commit line in single-commit mode' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree --no-commit-id "$c2" >out &&
	! head -1 out | grep "^[0-9a-f]\{40\}$"
'

test_expect_success 'diff-tree two commits shows changes between them' '
	cd repo &&
	c1=$(cat ../commit1) &&
	c2=$(cat ../commit2) &&
	git diff-tree "$c1" "$c2" >out &&
	grep "M" out &&
	grep "file.txt" out
'

test_expect_success 'diff-tree identical trees produces no output' '
	cd repo &&
	t1=$(cat ../tree1) &&
	git diff-tree "$t1" "$t1" >out &&
	test_must_be_empty out
'

test_expect_success 'diff-tree -r on nested adds shows full paths' '
	cd repo &&
	c2=$(cat ../commit2) &&
	c3=$(cat ../commit3) &&
	git diff-tree -r "$c2" "$c3" >out &&
	grep "A" out &&
	grep "sub/nested.txt" out
'

test_expect_success 'diff-tree --name-only on two commits' '
	cd repo &&
	c1=$(cat ../commit1) &&
	c2=$(cat ../commit2) &&
	git diff-tree --name-only "$c1" "$c2" >out &&
	grep "^file.txt$" out
'

test_expect_success 'diff-tree --name-status on two commits' '
	cd repo &&
	c1=$(cat ../commit1) &&
	c2=$(cat ../commit2) &&
	git diff-tree --name-status "$c1" "$c2" >out &&
	grep "^M" out &&
	grep "file.txt" out
'

test_expect_success 'diff-tree --stat on two commits' '
	cd repo &&
	c1=$(cat ../commit1) &&
	c2=$(cat ../commit2) &&
	git diff-tree --stat "$c1" "$c2" >out &&
	grep "file.txt" out &&
	grep "changed" out
'

test_expect_success 'diff-tree -p shows proper hunk headers' '
	cd repo &&
	c2=$(cat ../commit2) &&
	git diff-tree -r -p "$c2" >out &&
	grep "^@@" out
'

test_expect_success 'diff-tree --root on non-root commit still shows parent diff' '
	cd repo &&
	c3=$(cat ../commit3) &&
	git diff-tree -r --root "$c3" >out &&
	grep "sub/nested.txt" out
'

test_expect_success 'diff-tree --root shows A status for root commit' '
	cd repo &&
	c1=$(cat ../commit1) &&
	git diff-tree -r --root "$c1" >out &&
	grep "^:000000" out
'

test_done
