#!/bin/sh

test_description='grit diff with renames

Tests how grit reports file renames. With rename detection, renames
appear as R status with similarity. Tests cover pure renames,
rename+modify, rename via staging, and across commits.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup
# ============================================================

test_expect_success 'setup repo' '
	$REAL_GIT init rename-repo &&
	cd rename-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	printf "line1\nline2\nline3\n" >original.txt &&
	$REAL_GIT add original.txt &&
	$REAL_GIT commit -m "initial"
'

# ============================================================
# Pure rename (identical content)
# ============================================================

test_expect_success 'pure rename: git mv then commit' '
	cd rename-repo &&
	$REAL_GIT mv original.txt renamed.txt &&
	$REAL_GIT commit -m "rename"
'

test_expect_success 'diff between commits shows rename status' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R" actual | grep "original.txt" | grep "renamed.txt"
'

test_expect_success 'diff between commits shows 100% similarity' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R100" actual
'

test_expect_success 'diff patch shows rename from/to' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^rename from original.txt" actual &&
	grep "^rename to renamed.txt" actual
'

test_expect_success 'diff patch shows similarity index' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^similarity index" actual
'

test_expect_success 'diff --name-only lists new name' '
	cd rename-repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "renamed.txt" actual
'

test_expect_success 'diff --stat shows rename arrow' '
	cd rename-repo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "original.txt => renamed.txt" actual
'

test_expect_success 'diff --numstat shows zero additions/deletions for pure rename' '
	cd rename-repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "^0	0	renamed.txt" actual
'

# ============================================================
# Rename with content modification
# ============================================================

test_expect_success 'setup rename with modification' '
	cd rename-repo &&
	printf "line1\nmodified\nline3\n" >renamed.txt &&
	$REAL_GIT mv renamed.txt newname.txt &&
	printf "line1\nmodified\nline3\n" >newname.txt &&
	$REAL_GIT add newname.txt &&
	$REAL_GIT commit -m "rename+modify"
'

test_expect_success 'rename+modify: shows rename status' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R" actual | grep "newname.txt"
'

test_expect_success 'rename+modify: similarity is less than 100%' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R" actual | grep -v "R100"
'

test_expect_success 'rename+modify: patch shows content in new file' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "+modified" actual
'

# ============================================================
# Rename in staging area (--cached)
# ============================================================

test_expect_success 'setup staged rename' '
	cd rename-repo &&
	$REAL_GIT mv newname.txt staged.txt &&
	true
'

test_expect_success 'diff --cached shows staged rename' '
	cd rename-repo &&
	grit diff --cached --name-status >actual &&
	grep "^R" actual | grep "newname.txt" | grep "staged.txt"
'

test_expect_success 'diff --cached patch shows a/ and b/ paths' '
	cd rename-repo &&
	grit diff --cached >actual &&
	grep -- "--- a/newname.txt" actual &&
	grep -- "+++ b/staged.txt" actual
'

test_expect_success 'diff --cached --stat shows rename arrow' '
	cd rename-repo &&
	grit diff --cached --stat >actual &&
	grep "newname.txt => staged.txt" actual
'

test_expect_success 'commit staged rename' '
	cd rename-repo &&
	$REAL_GIT commit -m "staged rename"
'

# ============================================================
# Multiple renames in one commit
# ============================================================

test_expect_success 'setup multiple renames' '
	cd rename-repo &&
	echo "file a" >a.txt &&
	echo "file b" >b.txt &&
	echo "file c" >c.txt &&
	$REAL_GIT add a.txt b.txt c.txt &&
	$REAL_GIT commit -m "add abc" &&
	$REAL_GIT mv a.txt x.txt &&
	$REAL_GIT mv b.txt y.txt &&
	$REAL_GIT mv c.txt z.txt &&
	$REAL_GIT commit -m "rename abc to xyz"
'

test_expect_success 'multiple renames: all show as R status' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R.*a.txt.*x.txt" actual &&
	grep "^R.*b.txt.*y.txt" actual &&
	grep "^R.*c.txt.*z.txt" actual
'

test_expect_success 'multiple renames: --name-only lists 3 new names' '
	cd rename-repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'multiple renames: numstat has 3 lines' '
	cd rename-repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	test_line_count = 3 actual
'

# ============================================================
# Rename into subdirectory
# ============================================================

test_expect_success 'rename into subdirectory' '
	cd rename-repo &&
	mkdir -p subdir &&
	$REAL_GIT mv staged.txt subdir/moved.txt &&
	$REAL_GIT commit -m "move to subdir"
'

test_expect_success 'rename to subdir shows rename status' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^R.*staged.txt.*subdir/moved.txt" actual
'

test_expect_success 'rename to subdir: patch has correct a/ and b/ paths' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep -- "--- a/staged.txt" actual &&
	grep -- "+++ b/subdir/moved.txt" actual
'

# ============================================================
# diff-tree plumbing for renames
# ============================================================

test_expect_success 'diff-tree shows D and A for renamed file' '
	cd rename-repo &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	grep "D	staged.txt" actual &&
	grep "A	subdir/moved.txt" actual
'

test_expect_success 'diff-tree -p shows diff patch for rename' '
	cd rename-repo &&
	grit diff-tree -p HEAD~1 HEAD >actual &&
	grep "^diff --git" actual
'

test_done
