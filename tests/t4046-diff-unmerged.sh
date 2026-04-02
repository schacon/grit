#!/bin/sh

test_description='grit diff with unmerged index entries and merge conflicts

Tests diff behavior when the index contains higher-stage entries (unmerged),
as occurs during merge conflicts. Covers diff, diff --cached, diff-files,
diff-index, ls-files -u, and name-status during conflict states.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup: create a repo with a merge conflict using real git
# ============================================================

test_expect_success 'setup base repo with conflicting branches' '
	$REAL_GIT init unmerged &&
	cd unmerged &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "base content" >file.txt &&
	echo "no conflict" >safe.txt &&
	$REAL_GIT add file.txt safe.txt &&
	$REAL_GIT commit -m "base"
'

test_expect_success 'create branch1 with change to file.txt' '
	cd unmerged &&
	$REAL_GIT checkout -b branch1 &&
	echo "branch1 content" >file.txt &&
	echo "branch1 safe" >safe.txt &&
	$REAL_GIT add file.txt safe.txt &&
	$REAL_GIT commit -m "branch1 changes"
'

test_expect_success 'create branch2 with conflicting change' '
	cd unmerged &&
	$REAL_GIT checkout master &&
	$REAL_GIT checkout -b branch2 &&
	echo "branch2 content" >file.txt &&
	echo "branch2 safe" >safe.txt &&
	$REAL_GIT add file.txt safe.txt &&
	$REAL_GIT commit -m "branch2 changes"
'

test_expect_success 'merge branch1 into branch2 to create conflict' '
	cd unmerged &&
	test_must_fail $REAL_GIT merge branch1
'

# ============================================================
# ls-files -u: verify unmerged entries
# ============================================================

test_expect_success 'ls-files -u shows stage 1 (base) entry' '
	cd unmerged &&
	grit ls-files -u >actual &&
	grep "	file.txt$" actual | grep "1	" >stage1 &&
	test_line_count = 1 stage1
'

test_expect_success 'ls-files -u shows stage 2 (ours) entry' '
	cd unmerged &&
	grit ls-files -u >actual &&
	grep "	file.txt$" actual | grep "2	" >stage2 &&
	test_line_count = 1 stage2
'

test_expect_success 'ls-files -u shows stage 3 (theirs) entry' '
	cd unmerged &&
	grit ls-files -u >actual &&
	grep "	file.txt$" actual | grep "3	" >stage3 &&
	test_line_count = 1 stage3
'

test_expect_success 'ls-files -u shows all three stages for conflicted file' '
	cd unmerged &&
	grit ls-files -u >actual &&
	grep "file.txt" actual >conflict_entries &&
	test_line_count = 3 conflict_entries
'

test_expect_success 'ls-files -u shows entries for all conflicted files' '
	cd unmerged &&
	grit ls-files -u >actual &&
	grep "file.txt" actual &&
	grep "safe.txt" actual
'

# ============================================================
# diff-files: behavior during conflict
# ============================================================

test_expect_success 'diff-files shows U status for unmerged file' '
	cd unmerged &&
	grit diff-files >actual &&
	grep "U	file.txt" actual
'

test_expect_success 'diff-files raw output has zero OIDs for unmerged' '
	cd unmerged &&
	grit diff-files >actual &&
	grep "^:000000 000000 0\{40\} 0\{40\}" actual
'

test_expect_success 'diff-files lists all conflicted files' '
	cd unmerged &&
	grit diff-files >actual &&
	grep "file.txt" actual &&
	grep "safe.txt" actual
'

# ============================================================
# diff-index: behavior during conflict
# ============================================================

test_expect_success 'diff-index HEAD shows changes during conflict' '
	cd unmerged &&
	grit diff-index HEAD >actual &&
	test -s actual
'

test_expect_success 'diff-index HEAD lists file.txt during conflict' '
	cd unmerged &&
	grit diff-index HEAD >actual &&
	grep "file.txt" actual
'

# ============================================================
# diff --cached: behavior during conflict
# ============================================================

test_expect_success 'diff --cached produces output during conflict' '
	cd unmerged &&
	grit diff --cached >actual &&
	test -s actual
'

test_expect_success 'diff --cached shows deletion during conflict' '
	cd unmerged &&
	grit diff --cached >actual &&
	grep "^deleted file mode\|^---\|^-" actual
'

# ============================================================
# Resolve conflict, then verify clean state
# ============================================================

test_expect_success 'resolve all conflicts' '
	cd unmerged &&
	echo "resolved content" >file.txt &&
	echo "resolved safe" >safe.txt &&
	$REAL_GIT add file.txt safe.txt
'

test_expect_success 'ls-files -u is empty after resolving' '
	cd unmerged &&
	grit ls-files -u >actual &&
	! grep "file.txt" actual
'

test_expect_success 'diff-files is empty after staging resolution' '
	cd unmerged &&
	grit diff-files >actual &&
	test_line_count = 0 actual
'

test_expect_success 'diff --cached shows resolved changes vs HEAD' '
	cd unmerged &&
	grit diff --cached >actual &&
	grep "resolved content" actual
'

# ============================================================
# Multi-file conflict scenario
# ============================================================

test_expect_success 'setup multi-file conflict' '
	$REAL_GIT init multi &&
	cd multi &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo "a-base" >a.txt &&
	echo "b-base" >b.txt &&
	echo "c-base" >c.txt &&
	$REAL_GIT add a.txt b.txt c.txt &&
	$REAL_GIT commit -m "base" &&
	$REAL_GIT checkout -b left &&
	echo "a-left" >a.txt &&
	echo "b-left" >b.txt &&
	$REAL_GIT add a.txt b.txt &&
	$REAL_GIT commit -m "left" &&
	$REAL_GIT checkout master &&
	$REAL_GIT checkout -b right &&
	echo "a-right" >a.txt &&
	echo "b-right" >b.txt &&
	$REAL_GIT add a.txt b.txt &&
	$REAL_GIT commit -m "right" &&
	test_must_fail $REAL_GIT merge left
'

test_expect_success 'ls-files -u shows entries for both conflicted files' '
	cd multi &&
	grit ls-files -u >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual
'

test_expect_success 'ls-files -u does not show non-conflicted c.txt' '
	cd multi &&
	grit ls-files -u >actual &&
	! grep "c.txt" actual
'

test_expect_success 'ls-files -u shows 6 entries for 2 conflicted files' '
	cd multi &&
	grit ls-files -u >actual &&
	test_line_count = 6 actual
'

test_expect_success 'diff-files shows U for both conflicted files' '
	cd multi &&
	grit diff-files >actual &&
	grep "U	a.txt" actual &&
	grep "U	b.txt" actual
'

test_expect_success 'diff-files does not show clean c.txt' '
	cd multi &&
	grit diff-files >actual &&
	! grep "c.txt" actual
'

test_expect_success 'partially resolve: fix a.txt but leave b.txt' '
	cd multi &&
	echo "a-resolved" >a.txt &&
	$REAL_GIT add a.txt
'

test_expect_success 'ls-files -u no longer shows a.txt after partial resolve' '
	cd multi &&
	grit ls-files -u >actual &&
	! grep "a.txt" actual &&
	grep "b.txt" actual
'

test_expect_success 'diff-files only shows unresolved b.txt after partial resolve' '
	cd multi &&
	grit diff-files >actual &&
	! grep "a.txt" actual &&
	grep "U	b.txt" actual
'

test_done
