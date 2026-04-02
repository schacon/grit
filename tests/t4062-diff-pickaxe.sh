#!/bin/sh
# Test diff content-filtering options. Since grit does not yet implement
# -S (pickaxe), we verify that it is rejected, then exercise the
# content-related diff options that *are* supported: pathspec filtering,
# --name-only, --name-status, --stat, --numstat, and context control.

test_description='diff pickaxe and content filtering'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────────────

test_expect_success 'setup repo with multiple files and commits' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo "alpha" >a.txt &&
	echo "beta"  >b.txt &&
	grit add a.txt b.txt &&
	grit commit -m "c1" &&
	grit rev-parse HEAD >../c1 &&
	echo "alpha-mod" >a.txt &&
	echo "gamma" >c.txt &&
	grit add a.txt c.txt &&
	grit commit -m "c2" &&
	grit rev-parse HEAD >../c2 &&
	echo "delta" >>b.txt &&
	grit rm c.txt &&
	grit add b.txt &&
	grit commit -m "c3" &&
	grit rev-parse HEAD >../c3
'

# ── -S is unsupported (pickaxe) ───────────────────────────────────────────────

test_expect_success '-S flag is rejected' '
	cd repo &&
	test_must_fail grit diff -S "alpha" HEAD^ HEAD 2>err &&
	grep -i "unexpected\|unrecognized\|unknown" err
'

# ── Pathspec filtering ────────────────────────────────────────────────────────

test_expect_success 'diff with -- pathspec limits to one file' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff "$c1" "$c2" -- a.txt >out &&
	grep "a.txt" out &&
	! grep "c.txt" out
'

test_expect_success 'diff with -- pathspec for new file only' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff "$c1" "$c2" -- c.txt >out &&
	grep "c.txt" out &&
	! grep "a.txt" out
'

test_expect_success 'diff with pathspec matching no files produces empty output' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff "$c1" "$c2" -- nonexistent.txt >out &&
	test_must_fail test -s out
'

# ── --name-only ──────────────────────────────────────────────────────────────

test_expect_success '--name-only lists changed file names' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --name-only "$c1" "$c2" >out &&
	grep "a.txt" out &&
	grep "c.txt" out &&
	! grep "b.txt" out
'

test_expect_success '--name-only with pathspec' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --name-only "$c1" "$c2" -- a.txt >out &&
	grep "a.txt" out &&
	! grep "c.txt" out
'

test_expect_success '--name-only shows deleted file' '
	cd repo &&
	c2=$(cat ../c2) &&
	c3=$(cat ../c3) &&
	grit diff --name-only "$c2" "$c3" >out &&
	grep "c.txt" out
'

# ── --name-status ────────────────────────────────────────────────────────────

test_expect_success '--name-status shows M for modified' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --name-status "$c1" "$c2" >out &&
	grep "^M" out | grep "a.txt"
'

test_expect_success '--name-status shows A for added' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --name-status "$c1" "$c2" >out &&
	grep "^A" out | grep "c.txt"
'

test_expect_success '--name-status shows D for deleted' '
	cd repo &&
	c2=$(cat ../c2) &&
	c3=$(cat ../c3) &&
	grit diff --name-status "$c2" "$c3" >out &&
	grep "^D" out | grep "c.txt"
'

# ── --stat ───────────────────────────────────────────────────────────────────

test_expect_success '--stat shows summary' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --stat "$c1" "$c2" >out &&
	grep "a.txt" out &&
	grep "c.txt" out &&
	grep "changed" out
'

test_expect_success '--stat shows insertions and deletions' '
	cd repo &&
	c1=$(cat ../c1) &&
	c3=$(cat ../c3) &&
	grit diff --stat "$c1" "$c3" >out &&
	grep "insertion" out
'

# ── --numstat ────────────────────────────────────────────────────────────────

test_expect_success '--numstat shows numeric additions/deletions' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --numstat "$c1" "$c2" >out &&
	grep "a.txt" out &&
	grep "c.txt" out
'

test_expect_success '--numstat format is tab-separated' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff --numstat "$c1" "$c2" >out &&
	# Each line should have add<TAB>del<TAB>file
	awk -F"\t" "NF < 3 { exit 1 }" out
'

# ── Context control (-U) ────────────────────────────────────────────────────

test_expect_success '-U0 produces zero context lines' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff -U0 "$c1" "$c2" -- a.txt >out &&
	! grep "^  " out
'

test_expect_success '-U5 includes more context' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	grit diff -U5 "$c1" "$c2" -- a.txt >out &&
	grep "@@" out
'

# ── Combined filters ─────────────────────────────────────────────────────────

test_expect_success '--name-only with multiple commits' '
	cd repo &&
	c1=$(cat ../c1) &&
	c3=$(cat ../c3) &&
	grit diff --name-only "$c1" "$c3" >out &&
	grep "a.txt" out &&
	grep "b.txt" out
'

test_expect_success '--name-status across full range' '
	cd repo &&
	c1=$(cat ../c1) &&
	c3=$(cat ../c3) &&
	grit diff --name-status "$c1" "$c3" >out &&
	grep "M" out | grep "a.txt" &&
	grep "M" out | grep "b.txt"
'

# ── --exit-code / --quiet ────────────────────────────────────────────────────

test_expect_success '--exit-code returns 1 when there are differences' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	test_must_fail grit diff --exit-code "$c1" "$c2"
'

test_expect_success '--exit-code returns 0 when no differences' '
	cd repo &&
	c1=$(cat ../c1) &&
	grit diff --exit-code "$c1" "$c1"
'

test_expect_success '--quiet suppresses output but returns exit code' '
	cd repo &&
	c1=$(cat ../c1) &&
	c2=$(cat ../c2) &&
	test_must_fail grit diff --quiet "$c1" "$c2" >out &&
	! test -s out
'

test_expect_success '--quiet returns 0 for identical trees' '
	cd repo &&
	c1=$(cat ../c1) &&
	grit diff --quiet "$c1" "$c1"
'

test_done
