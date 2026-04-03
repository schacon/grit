#!/bin/sh

test_description='grit diff with binary files — rewrite, add, delete, mixed

Tests how grit handles binary files in diffs: binary detection,
stat/numstat output for binary files, mixing binary and text changes,
binary file deletion and addition, and complete binary rewrites.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup
# ============================================================

test_expect_success 'setup repo with binary and text files' '
	$REAL_GIT init binrepo &&
	cd binrepo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	printf "\x00\x01\x02\x03\x04\x05\x06\x07" >binary.dat &&
	echo "text content" >text.txt &&
	$REAL_GIT add binary.dat text.txt &&
	$REAL_GIT commit -m "initial with binary"
'

# ============================================================
# Binary file modification
# ============================================================

test_expect_success 'modify binary file' '
	cd binrepo &&
	printf "\x00\x01\x02\x03\x04\x05\x06\x08" >binary.dat &&
	$REAL_GIT add binary.dat &&
	$REAL_GIT commit -m "modify binary"
'

test_expect_success 'binary mod: name-status shows M' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*binary.dat" actual
'

test_expect_success 'binary mod: name-only lists file' '
	cd binrepo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "binary.dat" actual
'

test_expect_success 'binary mod: stat shows binary.dat' '
	cd binrepo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "binary.dat" actual
'

test_expect_success 'binary mod: numstat shows binary.dat' '
	cd binrepo &&
	grit diff --numstat HEAD~1 HEAD >actual &&
	grep "binary.dat" actual
'

test_expect_success 'binary mod: diff header present' '
	cd binrepo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^diff --git a/binary.dat b/binary.dat" actual
'

# ============================================================
# Complete binary rewrite
# ============================================================

test_expect_success 'completely rewrite binary to different content' '
	cd binrepo &&
	printf "\xff\xfe\xfd\xfc\xfb\xfa\xf9\xf8\xf7\xf6" >binary.dat &&
	$REAL_GIT add binary.dat &&
	$REAL_GIT commit -m "rewrite binary"
'

test_expect_success 'binary rewrite: name-status still M' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*binary.dat" actual
'

test_expect_success 'binary rewrite: diff-tree shows M' '
	cd binrepo &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	grep "M	binary.dat" actual
'

test_expect_success 'binary rewrite: OIDs differ' '
	cd binrepo &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	old_oid=$(awk "{print \$3}" actual | head -1) &&
	new_oid=$(awk "{print \$4}" actual | head -1) &&
	test "$old_oid" != "$new_oid"
'

# ============================================================
# Binary file addition
# ============================================================

test_expect_success 'add new binary file' '
	cd binrepo &&
	printf "\xde\xad\xbe\xef" >new-binary.bin &&
	$REAL_GIT add new-binary.bin &&
	$REAL_GIT commit -m "add new binary"
'

test_expect_success 'binary addition: name-status A' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^A.*new-binary.bin" actual
'

test_expect_success 'binary addition: diff shows new file mode' '
	cd binrepo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^new file mode" actual
'

# ============================================================
# Binary file deletion
# ============================================================

test_expect_success 'delete binary file' '
	cd binrepo &&
	$REAL_GIT rm new-binary.bin &&
	$REAL_GIT commit -m "delete new binary"
'

test_expect_success 'binary deletion: name-status D' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D.*new-binary.bin" actual
'

test_expect_success 'binary deletion: diff shows deleted file mode' '
	cd binrepo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^deleted file mode" actual
'

# ============================================================
# Mixed: binary + text changes in same commit
# ============================================================

test_expect_success 'modify both binary and text in same commit' '
	cd binrepo &&
	printf "\xaa\xbb\xcc\xdd" >binary.dat &&
	echo "modified text" >text.txt &&
	$REAL_GIT add binary.dat text.txt &&
	$REAL_GIT commit -m "modify both"
'

test_expect_success 'mixed: name-status lists both files' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*binary.dat" actual &&
	grep "^M.*text.txt" actual
'

test_expect_success 'mixed: name-only lists both files' '
	cd binrepo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "binary.dat" actual &&
	grep "text.txt" actual
'

test_expect_success 'mixed: stat lists both files' '
	cd binrepo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "binary.dat" actual &&
	grep "text.txt" actual
'

test_expect_success 'mixed: diff has headers for both files' '
	cd binrepo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^diff --git a/binary.dat" actual &&
	grep "^diff --git a/text.txt" actual
'

test_expect_success 'mixed: text diff shows proper unified output' '
	cd binrepo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-text content$" actual &&
	grep "^+modified text$" actual
'

# ============================================================
# Binary rename (shows as D+A without -M)
# ============================================================

test_expect_success 'rename binary file' '
	cd binrepo &&
	$REAL_GIT mv binary.dat moved.dat &&
	$REAL_GIT commit -m "rename binary"
'

test_expect_success 'binary rename: detected as rename' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "R.*binary.dat.*moved.dat" actual
'

# ============================================================
# Large binary changes spanning multiple commits
# ============================================================

test_expect_success 'setup: create larger binary' '
	cd binrepo &&
	dd if=/dev/urandom of=large.bin bs=1024 count=4 2>/dev/null &&
	$REAL_GIT add large.bin &&
	$REAL_GIT commit -m "add large binary"
'

test_expect_success 'rewrite large binary' '
	cd binrepo &&
	dd if=/dev/urandom of=large.bin bs=1024 count=4 2>/dev/null &&
	$REAL_GIT add large.bin &&
	$REAL_GIT commit -m "rewrite large binary"
'

test_expect_success 'large binary rewrite: name-status M' '
	cd binrepo &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^M.*large.bin" actual
'

test_expect_success 'large binary rewrite: diff-tree shows M' '
	cd binrepo &&
	grit diff-tree -r HEAD~1 HEAD >actual &&
	grep "M	large.bin" actual
'

test_done
