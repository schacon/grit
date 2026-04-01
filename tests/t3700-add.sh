#!/bin/sh
# Ported from git/t/t3700-add.sh
# Tests for 'grit add'.

test_description='grit add'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo
'

test_expect_success 'add a single file' '
	cd repo &&
	echo "hello" >file1.txt &&
	git add file1.txt &&
	git ls-files --stage >actual &&
	grep "file1.txt" actual
'

test_expect_success 'add multiple files' '
	cd repo &&
	echo "world" >file2.txt &&
	echo "foo" >file3.txt &&
	git add file2.txt file3.txt &&
	git ls-files --stage >actual &&
	grep "file2.txt" actual &&
	grep "file3.txt" actual
'

test_expect_success 'add all with dot' '
	cd repo &&
	echo "new" >file4.txt &&
	git add . &&
	git ls-files --stage >actual &&
	grep "file4.txt" actual
'

test_expect_success 'add files in subdirectory' '
	cd repo &&
	mkdir -p subdir &&
	echo "nested" >subdir/deep.txt &&
	git add subdir/deep.txt &&
	git ls-files --stage >actual &&
	grep "subdir/deep.txt" actual
'

test_expect_success 'add directory recursively' '
	cd repo &&
	mkdir -p dir2 &&
	echo "a" >dir2/a.txt &&
	echo "b" >dir2/b.txt &&
	git add dir2 &&
	git ls-files --stage >actual &&
	grep "dir2/a.txt" actual &&
	grep "dir2/b.txt" actual
'

test_expect_success 'add updates modified file' '
	cd repo &&
	echo "updated" >file1.txt &&
	git add file1.txt &&
	git ls-files --stage >actual &&
	# The OID should have changed
	grep "file1.txt" actual >line &&
	! grep "ce013625030ba8dba906f756967f9e9ca394464a" line
'

test_expect_success 'add -A removes deleted files from index' '
	cd repo &&
	rm file3.txt &&
	git add -A &&
	git ls-files --stage >actual &&
	! grep "file3.txt" actual
'

test_expect_success 'add -u updates tracked files only' '
	cd repo &&
	echo "untracked" >untracked.txt &&
	echo "modified" >file1.txt &&
	git add -u &&
	git ls-files --stage >actual &&
	! grep "untracked.txt" actual &&
	grep "file1.txt" actual
'

test_expect_success 'add -v is verbose' '
	cd repo &&
	echo "verbosetest" >vfile.txt &&
	git add -v vfile.txt 2>stderr &&
	grep "add" stderr
'

test_expect_success 'add -n dry run does not modify index' '
	cd repo &&
	echo "dryrun" >dryfile.txt &&
	git ls-files --stage >before &&
	git add -n dryfile.txt 2>/dev/null &&
	git ls-files --stage >after &&
	test_cmp before after
'

test_expect_success 'add nonexistent file fails' '
	cd repo &&
	! git add nonexistent.txt 2>/dev/null
'

test_done
