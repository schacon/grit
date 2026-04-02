#!/bin/sh

test_description='grit diff with renames (without -M rename detection)

Tests how grit reports file renames. Without -M support, renames appear
as a deletion of the old path and addition of the new path. Tests cover
pure renames, rename+modify, rename via staging, and across commits.'

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

test_expect_success 'diff between commits shows old file deleted' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D" actual | grep "original.txt"
'

test_expect_success 'diff between commits shows new file added' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^A" actual | grep "renamed.txt"
'

test_expect_success 'diff patch shows deleted file mode for original' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^deleted file mode" actual
'

test_expect_success 'diff patch shows new file mode for renamed' '
	cd rename-repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^new file mode" actual
'

test_expect_success 'diff --name-only lists both files' '
	cd rename-repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "original.txt" actual &&
	grep "renamed.txt" actual
'

test_expect_success 'diff --stat shows both files' '
	cd rename-repo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "original.txt" actual &&
	grep "renamed.txt" actual
'

test_expect_success 'diff --numstat shows deletions for original and additions for renamed' '
	cd rename-repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "original.txt" actual &&
	grep "renamed.txt" actual
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

test_expect_success 'rename+modify: old file shows as deleted' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D" actual | grep "renamed.txt"
'

test_expect_success 'rename+modify: new file shows as added' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^A" actual | grep "newname.txt"
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

test_expect_success 'diff --cached shows staged rename as D+A' '
	cd rename-repo &&
	grit diff --cached --name-status >actual &&
	grep "^D" actual | grep "newname.txt" &&
	grep "^A" actual | grep "staged.txt"
'

test_expect_success 'diff --cached patch shows deletion header for old' '
	cd rename-repo &&
	grit diff --cached >actual &&
	grep -- "--- a/newname.txt" actual
'

test_expect_success 'diff --cached patch shows addition header for new' '
	cd rename-repo &&
	grit diff --cached >actual &&
	grep "+++ b/staged.txt" actual
'

test_expect_success 'diff --cached --stat shows both files' '
	cd rename-repo &&
	grit diff --cached --stat >actual &&
	grep "newname.txt" actual &&
	grep "staged.txt" actual
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

test_expect_success 'multiple renames: all old files show as deleted' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D.*a.txt" actual &&
	grep "^D.*b.txt" actual &&
	grep "^D.*c.txt" actual
'

test_expect_success 'multiple renames: all new files show as added' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^A.*x.txt" actual &&
	grep "^A.*y.txt" actual &&
	grep "^A.*z.txt" actual
'

test_expect_success 'multiple renames: --name-only lists 6 files' '
	cd rename-repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	test_line_count = 6 actual
'

test_expect_success 'multiple renames: numstat has 6 lines' '
	cd rename-repo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	test_line_count = 6 actual
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

test_expect_success 'rename to subdir shows correct paths' '
	cd rename-repo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D.*staged.txt" actual &&
	grep "^A.*subdir/moved.txt" actual
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
