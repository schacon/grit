#!/bin/sh

test_description='grit diff with complete file rewrites

Tests how grit handles files whose content is completely or nearly
completely rewritten. Without -B (break/rewrite) support, grit shows
these as standard modifications with all old lines deleted and all
new lines added. Tests cover full rewrites, near-rewrites, and
interaction with --stat, --numstat, --name-status.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup
# ============================================================

test_expect_success 'setup repo with initial content' '
	$REAL_GIT init rewrite &&
	cd rewrite &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	printf "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "initial"
'

# ============================================================
# Complete rewrite: all content replaced
# ============================================================

test_expect_success 'completely rewrite file content' '
	cd rewrite &&
	printf "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "complete rewrite"
'

test_expect_success 'complete rewrite: name-status shows M (not D+A)' '
	cd rewrite &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*file.txt" actual
'

test_expect_success 'complete rewrite: patch shows all old lines deleted' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-line1$" actual &&
	grep "^-line10$" actual
'

test_expect_success 'complete rewrite: patch shows all new lines added' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^+alpha$" actual &&
	grep "^+kappa$" actual
'

test_expect_success 'complete rewrite: no context lines (nothing shared)' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	! grep "^ " actual
'

test_expect_success 'complete rewrite: stat shows correct counts' '
	cd rewrite &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "file.txt" actual &&
	grep "10.*+" actual &&
	grep "10.*-" actual
'

test_expect_success 'complete rewrite: numstat shows 10 add 10 del' '
	cd rewrite &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "^10	10	file.txt$" actual
'

test_expect_success 'complete rewrite: single diff --git header' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	count=$(grep -c "^diff --git" actual) &&
	test "$count" = 1
'

test_expect_success 'complete rewrite: single hunk' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	count=$(grep -c "^@@" actual) &&
	test "$count" = 1
'

# ============================================================
# Near-rewrite: most content changed, a few lines survive
# ============================================================

test_expect_success 'near-rewrite: keep 2 of 10 lines' '
	cd rewrite &&
	printf "alpha\nNEW2\nNEW3\nNEW4\nepsilon\nNEW6\nNEW7\nNEW8\nNEW9\nNEW10\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "near rewrite"
'

test_expect_success 'near-rewrite: still shows as M' '
	cd rewrite &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*file.txt" actual
'

test_expect_success 'near-rewrite: patch shows surviving lines as context' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^ alpha$" actual &&
	grep "^ epsilon$" actual
'

test_expect_success 'near-rewrite: numstat shows 8 additions 8 deletions' '
	cd rewrite &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "^8	8	file.txt$" actual
'

# ============================================================
# Rewrite to empty file
# ============================================================

test_expect_success 'truncate file to empty' '
	cd rewrite &&
	>file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "empty file"
'

test_expect_success 'rewrite to empty: name-status M' '
	cd rewrite &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*file.txt" actual
'

test_expect_success 'rewrite to empty: numstat shows 0 add N del' '
	cd rewrite &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "^0	10	file.txt$" actual
'

test_expect_success 'rewrite to empty: patch has only deletions' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-" actual | grep -v "^---" >dels &&
	test -s dels &&
	! grep "^+" actual | grep -v "^+++"
'

# ============================================================
# Rewrite from empty
# ============================================================

test_expect_success 'fill previously-empty file' '
	cd rewrite &&
	printf "new1\nnew2\nnew3\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "fill empty"
'

test_expect_success 'rewrite from empty: numstat shows 3 add 0 del' '
	cd rewrite &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "^3	0	file.txt$" actual
'

test_expect_success 'rewrite from empty: patch has only additions' '
	cd rewrite &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^+new1$" actual &&
	grep "^+new3$" actual
'

# ============================================================
# Multiple files, one rewritten one stable
# ============================================================

test_expect_success 'setup: add stable file' '
	cd rewrite &&
	echo "stable" >stable.txt &&
	$REAL_GIT add stable.txt &&
	$REAL_GIT commit -m "add stable"
'

test_expect_success 'rewrite one file, leave other unchanged' '
	cd rewrite &&
	printf "completely\ndifferent\ncontent\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "rewrite file.txt again"
'

test_expect_success 'only rewritten file appears in name-only' '
	cd rewrite &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "file.txt" actual &&
	! grep "stable.txt" actual
'

test_expect_success 'stat output only shows changed file' '
	cd rewrite &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "file.txt" actual &&
	! grep "stable.txt" actual
'

# ============================================================
# diff-tree plumbing for rewrites
# ============================================================

test_expect_success 'diff-tree shows M for rewritten file' '
	cd rewrite &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	grep "M	file.txt" actual
'

test_expect_success 'diff-tree -p shows patch for rewritten file' '
	cd rewrite &&
	grit diff-tree -p HEAD~1 HEAD >actual &&
	grep "^diff --git a/file.txt b/file.txt" actual &&
	grep "^@@" actual
'

test_expect_success 'diff-tree OIDs are different for rewrite' '
	cd rewrite &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	old_oid=$(awk "{print \$3}" actual | head -1) &&
	new_oid=$(awk "{print \$4}" actual | head -1) &&
	test "$old_oid" != "$new_oid"
'

test_done
