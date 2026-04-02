#!/bin/sh
# Test rev-list ordering, counting, range queries, and history traversal
# options: --count, --reverse, --max-count, --first-parent, --topo-order,
# --date-order, commit ranges, and exclusions.

test_description='rev-list ordering, ranges, and history traversal'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=""
for p in /usr/bin/git /usr/local/bin/git; do
	if test -x "$p"; then
		REAL_GIT="$p"
		break
	fi
done
if test -z "$REAL_GIT"; then
	echo "SKIP: real git not found" >&2
	exit 0
fi

test_expect_success 'setup: create linear history' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "A" >file.txt &&
	grit add file.txt &&
	GIT_COMMITTER_DATE="1000000000 +0000" GIT_AUTHOR_DATE="1000000000 +0000" \
		grit commit -m "commit-A" &&
	echo "B" >file.txt &&
	grit add file.txt &&
	GIT_COMMITTER_DATE="1000000100 +0000" GIT_AUTHOR_DATE="1000000100 +0000" \
		grit commit -m "commit-B" &&
	echo "C" >file.txt &&
	grit add file.txt &&
	GIT_COMMITTER_DATE="1000000200 +0000" GIT_AUTHOR_DATE="1000000200 +0000" \
		grit commit -m "commit-C"
'

test_expect_success 'rev-list HEAD lists all commits' '
	cd repo &&
	grit rev-list HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list --count HEAD returns 3' '
	cd repo &&
	COUNT=$(grit rev-list --count HEAD) &&
	test "$COUNT" = "3"
'

test_expect_success 'rev-list --max-count=1 returns one commit' '
	cd repo &&
	grit rev-list --max-count=1 HEAD >actual &&
	test_line_count = 1 actual
'

test_expect_success 'rev-list --max-count=2 returns two commits' '
	cd repo &&
	grit rev-list --max-count=2 HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list --max-count=0 returns no commits' '
	cd repo &&
	grit rev-list --max-count=0 HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'rev-list --reverse reverses output' '
	cd repo &&
	grit rev-list HEAD >forward &&
	grit rev-list --reverse HEAD >backward &&
	# first of forward = last of backward
	HEAD_FWD=$(head -1 forward) &&
	HEAD_BWD=$(tail -1 backward) &&
	test "$HEAD_FWD" = "$HEAD_BWD" &&
	# last of forward = first of backward
	TAIL_FWD=$(tail -1 forward) &&
	TAIL_BWD=$(head -1 backward) &&
	test "$TAIL_FWD" = "$TAIL_BWD"
'

test_expect_success 'rev-list --reverse --max-count=2 returns 2 newest reversed' '
	cd repo &&
	grit rev-list --reverse --max-count=2 HEAD >actual &&
	test_line_count = 2 actual &&
	# --max-count limits first, then --reverse flips
	grit rev-list --max-count=2 HEAD >fwd &&
	LAST_FWD=$(tail -1 fwd) &&
	FIRST_REV=$(head -1 actual) &&
	test "$LAST_FWD" = "$FIRST_REV"
'

# ── range queries ──

test_expect_success 'rev-list HEAD~1..HEAD returns 1 commit' '
	cd repo &&
	grit rev-list HEAD~1..HEAD >actual &&
	test_line_count = 1 actual
'

test_expect_success 'rev-list HEAD~2..HEAD returns 2 commits' '
	cd repo &&
	grit rev-list HEAD~2..HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list A ^B equivalent to B..A' '
	cd repo &&
	grit rev-list HEAD ^HEAD~2 >excl &&
	grit rev-list HEAD~2..HEAD >range &&
	test_cmp excl range
'

test_expect_success 'rev-list with explicit commit OIDs' '
	cd repo &&
	A=$(grit rev-list --reverse HEAD | head -1) &&
	C=$(grit rev-list --max-count=1 HEAD) &&
	grit rev-list $C ^$A >actual &&
	test_line_count = 2 actual
'

# ── branches and merges ──

test_expect_success 'setup: create branch and merge' '
	cd repo &&
	"$REAL_GIT" checkout -b side &&
	echo "side1" >side.txt &&
	grit add side.txt &&
	GIT_COMMITTER_DATE="1000000300 +0000" GIT_AUTHOR_DATE="1000000300 +0000" \
		grit commit -m "side-1" &&
	echo "side2" >>side.txt &&
	grit add side.txt &&
	GIT_COMMITTER_DATE="1000000400 +0000" GIT_AUTHOR_DATE="1000000400 +0000" \
		grit commit -m "side-2" &&
	"$REAL_GIT" checkout master &&
	echo "D" >file.txt &&
	grit add file.txt &&
	GIT_COMMITTER_DATE="1000000350 +0000" GIT_AUTHOR_DATE="1000000350 +0000" \
		grit commit -m "commit-D" &&
	"$REAL_GIT" merge side --no-edit
'

test_expect_success 'rev-list HEAD after merge counts all' '
	cd repo &&
	COUNT=$(grit rev-list --count HEAD) &&
	test "$COUNT" -ge 6
'

test_expect_success 'rev-list --first-parent HEAD after merge' '
	cd repo &&
	FP=$(grit rev-list --first-parent --count HEAD) &&
	ALL=$(grit rev-list --count HEAD) &&
	test "$FP" -lt "$ALL"
'

test_expect_success 'rev-list --first-parent does not include side commits' '
	cd repo &&
	grit rev-list --first-parent HEAD >fp &&
	SIDE_TIP=$(grit show-ref --hash refs/heads/side) &&
	! grep "$SIDE_TIP" fp
'

test_expect_success 'rev-list --topo-order HEAD' '
	cd repo &&
	grit rev-list --topo-order HEAD >topo &&
	COUNT=$(wc -l <topo | tr -d " ") &&
	test "$COUNT" -ge 6
'

test_expect_success 'rev-list --date-order HEAD' '
	cd repo &&
	grit rev-list --date-order HEAD >dated &&
	COUNT=$(wc -l <dated | tr -d " ") &&
	test "$COUNT" -ge 6
'

test_expect_success 'topo-order and date-order have same commits but may differ in order' '
	cd repo &&
	grit rev-list --topo-order HEAD | sort >topo_sorted &&
	grit rev-list --date-order HEAD | sort >date_sorted &&
	test_cmp topo_sorted date_sorted
'

test_expect_success 'rev-list --count with --first-parent' '
	cd repo &&
	FP=$(grit rev-list --first-parent --count HEAD) &&
	test "$FP" -ge 4 &&
	test "$FP" -le 6
'

# ── multiple branches in rev-list ──

test_expect_success 'rev-list side shows side commits' '
	cd repo &&
	grit rev-list side >side_list &&
	COUNT=$(wc -l <side_list | tr -d " ") &&
	test "$COUNT" = "5"
'

test_expect_success 'rev-list master ^side shows master-only commits' '
	cd repo &&
	grit rev-list master ^side >master_only &&
	COUNT=$(wc -l <master_only | tr -d " ") &&
	test "$COUNT" -ge 1
'

# ── deeper history ──

test_expect_success 'setup: add more history' '
	cd repo &&
	for i in 1 2 3 4 5; do
		echo "extra-$i" >extra-$i.txt &&
		grit add extra-$i.txt &&
		grit commit -m "extra-$i" || return 1
	done
'

test_expect_success 'rev-list --count increased by 5' '
	cd repo &&
	COUNT=$(grit rev-list --count HEAD) &&
	test "$COUNT" -ge 11
'

test_expect_success 'rev-list --max-count=3 with deep history' '
	cd repo &&
	grit rev-list --max-count=3 HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list --reverse --max-count=1 gives HEAD' '
	cd repo &&
	HEAD_OID=$(grit rev-list --max-count=1 HEAD) &&
	grit rev-list --reverse --max-count=1 HEAD >actual &&
	FIRST=$(cat actual) &&
	test "$FIRST" = "$HEAD_OID"
'

test_expect_success 'rev-list HEAD~5..HEAD returns 5 commits' '
	cd repo &&
	grit rev-list HEAD~5..HEAD >actual &&
	test_line_count = 5 actual
'

test_expect_success 'rev-list --count HEAD~5..HEAD returns 5' '
	cd repo &&
	COUNT=$(grit rev-list --count HEAD~5..HEAD) &&
	test "$COUNT" = "5"
'

test_expect_success 'rev-list --reverse HEAD~3..HEAD' '
	cd repo &&
	grit rev-list HEAD~3..HEAD >fwd &&
	grit rev-list --reverse HEAD~3..HEAD >rev &&
	FWD_FIRST=$(head -1 fwd) &&
	REV_LAST=$(tail -1 rev) &&
	test "$FWD_FIRST" = "$REV_LAST"
'

test_expect_success 'rev-list --topo-order --max-count=3' '
	cd repo &&
	grit rev-list --topo-order --max-count=3 HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list with invalid ref fails' '
	cd repo &&
	test_must_fail grit rev-list nonexistent-ref
'

test_expect_success 'rev-list --count with range and --first-parent' '
	cd repo &&
	FP=$(grit rev-list --first-parent --count HEAD~3..HEAD) &&
	test "$FP" = "3"
'

test_done
