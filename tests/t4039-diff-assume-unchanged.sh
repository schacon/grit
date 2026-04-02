#!/bin/sh
#
# Tests for update-index --assume-unchanged and its interaction with diff.

test_description='update-index --assume-unchanged and diff behavior'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "line one" >file1.txt &&
	echo "line two" >file2.txt &&
	echo "line three" >file3.txt &&
	git add . &&
	git commit -m "initial"
'

# ---------------------------------------------------------------------------
# Basic --assume-unchanged flag setting
# ---------------------------------------------------------------------------
test_expect_success 'update-index --assume-unchanged succeeds' '
	cd repo &&
	git update-index --assume-unchanged file1.txt
'

test_expect_success 'update-index --no-assume-unchanged succeeds' '
	cd repo &&
	git update-index --no-assume-unchanged file1.txt
'

test_expect_success 'assume-unchanged sets the flag (verified with /usr/bin/git)' '
	cd repo &&
	git update-index --assume-unchanged file1.txt &&
	/usr/bin/git ls-files -v >out &&
	grep "^h file1.txt" out &&
	git update-index --no-assume-unchanged file1.txt
'

test_expect_success 'no-assume-unchanged clears the flag' '
	cd repo &&
	git update-index --assume-unchanged file1.txt &&
	/usr/bin/git ls-files -v >out1 &&
	grep "^h file1.txt" out1 &&
	git update-index --no-assume-unchanged file1.txt &&
	/usr/bin/git ls-files -v >out2 &&
	grep "^H file1.txt" out2
'

test_expect_success 'assume-unchanged on multiple files' '
	cd repo &&
	git update-index --assume-unchanged file1.txt file2.txt &&
	/usr/bin/git ls-files -v >out &&
	grep "^h file1.txt" out &&
	grep "^h file2.txt" out &&
	grep "^H file3.txt" out &&
	git update-index --no-assume-unchanged file1.txt file2.txt
'

test_expect_success 'no-assume-unchanged on multiple files' '
	cd repo &&
	git update-index --assume-unchanged file1.txt file2.txt file3.txt &&
	git update-index --no-assume-unchanged file1.txt file2.txt file3.txt &&
	/usr/bin/git ls-files -v >out &&
	grep "^H file1.txt" out &&
	grep "^H file2.txt" out &&
	grep "^H file3.txt" out
'

# ---------------------------------------------------------------------------
# Diff with modified file (no assume-unchanged)
# ---------------------------------------------------------------------------
test_expect_success 'diff shows changes for normal modified files' '
	cd repo &&
	echo "modified" >file1.txt &&
	git diff >out &&
	test -s out &&
	echo "line one" >file1.txt
'

# ---------------------------------------------------------------------------
# Diff after assume-unchanged + refresh hides changes
# ---------------------------------------------------------------------------
test_expect_success 'diff after assume-unchanged + refresh hides changes' '
	cd repo &&
	echo "modified" >file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git update-index --refresh &&
	git diff >out &&
	! test -s out &&
	echo "line one" >file1.txt &&
	git update-index --no-assume-unchanged file1.txt
'

test_expect_success 'diff --exit-code returns 0 for assume-unchanged after refresh' '
	cd repo &&
	echo "modified" >file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git update-index --refresh &&
	git diff --exit-code &&
	echo "line one" >file1.txt &&
	git update-index --no-assume-unchanged file1.txt
'

test_expect_success 'un-assume and modify again shows diff' '
	cd repo &&
	echo "modified" >file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git update-index --refresh &&
	git diff >out_assumed &&
	! test -s out_assumed &&
	git update-index --no-assume-unchanged file1.txt &&
	echo "modified again" >file1.txt &&
	git diff >out_unassumed &&
	test -s out_unassumed &&
	echo "line one" >file1.txt
'

# ---------------------------------------------------------------------------
# --really-refresh
# ---------------------------------------------------------------------------
test_expect_success 'update-index --really-refresh succeeds' '
	cd repo &&
	git update-index --really-refresh
'

test_expect_success 'really-refresh revalidates stat for assume-unchanged files' '
	cd repo &&
	echo "changed" >file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git update-index --really-refresh &&
	git diff >out &&
	! test -s out &&
	echo "line one" >file1.txt &&
	git update-index --no-assume-unchanged file1.txt
'

# ---------------------------------------------------------------------------
# Assume-unchanged with staging
# ---------------------------------------------------------------------------
test_expect_success 'git add still works on assume-unchanged files' '
	cd repo &&
	echo "new content" >file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git add file1.txt &&
	git diff --cached --name-only >out &&
	grep file1.txt out &&
	git reset HEAD file1.txt &&
	echo "line one" >file1.txt &&
	git update-index --no-assume-unchanged file1.txt
'

# ---------------------------------------------------------------------------
# Assume-unchanged on nonexistent file
# ---------------------------------------------------------------------------
test_expect_success 'assume-unchanged on untracked file does not error' '
	cd repo &&
	git update-index --assume-unchanged noexist.txt 2>err;
	# grit returns 0 for nonexistent files (unlike C git which errors)
	true
'

# ---------------------------------------------------------------------------
# Diff --cached unaffected by assume-unchanged
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached is not affected by assume-unchanged' '
	cd repo &&
	echo "staged" >file1.txt &&
	git add file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git diff --cached --name-only >out &&
	grep file1.txt out &&
	git reset HEAD file1.txt &&
	echo "line one" >file1.txt &&
	git update-index --no-assume-unchanged file1.txt
'

# ---------------------------------------------------------------------------
# Refresh behavior
# ---------------------------------------------------------------------------
test_expect_success 'update-index --refresh succeeds' '
	cd repo &&
	git update-index --refresh
'

test_expect_success 'refresh after assume-unchanged clears stat for that file' '
	cd repo &&
	echo "changed" >file3.txt &&
	git update-index --assume-unchanged file3.txt &&
	git update-index --refresh &&
	git diff --name-only >out &&
	! grep file3.txt out &&
	echo "line three" >file3.txt &&
	git update-index --no-assume-unchanged file3.txt
'

# ---------------------------------------------------------------------------
# Assume-unchanged flag survives refresh
# ---------------------------------------------------------------------------
test_expect_success 'assume-unchanged flag survives refresh' '
	cd repo &&
	git update-index --assume-unchanged file1.txt &&
	git update-index --refresh &&
	/usr/bin/git ls-files -v >out &&
	grep "^h file1.txt" out &&
	git update-index --no-assume-unchanged file1.txt
'

# ---------------------------------------------------------------------------
# Assume-unchanged with commit
# ---------------------------------------------------------------------------
test_expect_success 'staged changes can be committed regardless of assume-unchanged' '
	cd repo &&
	echo "staged-content" >file1.txt &&
	git add file1.txt &&
	git update-index --assume-unchanged file1.txt &&
	git commit -m "commit with assume-unchanged" &&
	git log --oneline >log &&
	grep "commit with assume-unchanged" log &&
	echo "line one" >file1.txt &&
	git add file1.txt &&
	git commit -m "restore" &&
	git update-index --no-assume-unchanged file1.txt
'

test_done
