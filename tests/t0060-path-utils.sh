#!/bin/sh
# Test path normalization and relative path computation.

test_description='grit path utility operations (rev-parse, ls-files path handling)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: rev-parse --git-dir and --show-toplevel
###########################################################################

test_expect_success 'setup: init repo with nested dirs' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test User" &&
	mkdir -p sub/deep/deeper &&
	echo content >file.txt &&
	echo nested >sub/file.txt &&
	echo deep >sub/deep/file.txt &&
	echo deeper >sub/deep/deeper/file.txt &&
	grit add . &&
	grit commit -m "initial"
'

test_expect_success 'rev-parse --git-dir from repo root' '
	cd repo &&
	result=$(grit rev-parse --git-dir) &&
	test "$result" = ".git"
'

test_expect_success 'rev-parse --git-dir from subdirectory' '
	cd repo/sub &&
	result=$(grit rev-parse --git-dir) &&
	test "$result" = "../.git" || test "$result" = "$(cd ../.. && pwd)/repo/.git"
'

test_expect_success 'rev-parse --show-toplevel from root' '
	cd repo &&
	result=$(grit rev-parse --show-toplevel) &&
	expected=$(pwd) &&
	test "$result" = "$expected"
'

test_expect_success 'rev-parse --show-toplevel from subdirectory' '
	cd repo/sub/deep &&
	result=$(grit rev-parse --show-toplevel) &&
	expected=$(cd ../.. && pwd) &&
	test "$result" = "$expected"
'

test_expect_success 'rev-parse --show-toplevel from deeply nested dir' '
	cd repo/sub/deep/deeper &&
	result=$(grit rev-parse --show-toplevel) &&
	expected=$(cd ../../.. && pwd) &&
	test "$result" = "$expected"
'

###########################################################################
# Section 2: ls-files with pathspecs
###########################################################################

test_expect_success 'ls-files with path restricts to that directory' '
	cd repo &&
	grit ls-files sub/ >actual &&
	grep "sub/" actual &&
	! grep "^file.txt$" actual
'

test_expect_success 'ls-files from subdirectory shows relative paths' '
	cd repo/sub &&
	grit ls-files >actual &&
	cat actual >../sub-ls-files
'

test_expect_success 'ls-files with explicit file pathspec' '
	cd repo &&
	grit ls-files file.txt >actual &&
	test "$(cat actual)" = "file.txt"
'

test_expect_success 'ls-files with nested pathspec' '
	cd repo &&
	grit ls-files sub/deep/ >actual &&
	grep "sub/deep/file.txt" actual &&
	grep "sub/deep/deeper/file.txt" actual
'

test_expect_success 'ls-files does not list non-existent pathspec' '
	cd repo &&
	grit ls-files nonexistent >actual &&
	test_must_be_empty actual
'

###########################################################################
# Section 3: Paths with special characters
###########################################################################

test_expect_success 'add and ls-files with spaces in path' '
	cd repo &&
	mkdir -p "dir with spaces" &&
	echo content >"dir with spaces/file name.txt" &&
	grit add "dir with spaces/file name.txt" &&
	grit ls-files >actual &&
	grep "dir with spaces/file name.txt" actual
'

test_expect_success 'add and ls-files with dashes in filename' '
	cd repo &&
	echo content >file-with-dashes.txt &&
	grit add file-with-dashes.txt &&
	grit ls-files >actual &&
	grep "file-with-dashes.txt" actual
'

test_expect_success 'add and ls-files with dots in directory names' '
	cd repo &&
	mkdir -p "dir.with.dots" &&
	echo content >"dir.with.dots/test.txt" &&
	grit add "dir.with.dots/test.txt" &&
	grit ls-files >actual &&
	grep "dir.with.dots/test.txt" actual
'

test_expect_success 'add and ls-files with underscores' '
	cd repo &&
	echo content >under_score_file.txt &&
	grit add under_score_file.txt &&
	grit ls-files >actual &&
	grep "under_score_file.txt" actual
'

###########################################################################
# Section 4: ls-tree path handling
###########################################################################

test_expect_success 'ls-tree shows root tree entries' '
	cd repo &&
	grit commit -m "more files" &&
	tree=$(grit rev-parse "HEAD^{tree}") &&
	grit ls-tree "$tree" >actual &&
	grep "file.txt" actual &&
	grep "sub" actual
'

test_expect_success 'ls-tree with path restricts output' '
	cd repo &&
	tree=$(grit rev-parse "HEAD^{tree}") &&
	grit ls-tree "$tree" sub >actual &&
	grep "sub" actual
'

test_expect_success 'ls-tree -r recurses into directories' '
	cd repo &&
	tree=$(grit rev-parse "HEAD^{tree}") &&
	grit ls-tree -r "$tree" >actual &&
	grep "sub/deep/file.txt" actual &&
	grep "sub/deep/deeper/file.txt" actual
'

test_expect_success 'ls-tree -r with path restriction' '
	cd repo &&
	tree=$(grit rev-parse "HEAD^{tree}") &&
	grit ls-tree -r "$tree" sub >actual &&
	grep "sub/deep/file.txt" actual &&
	grep "sub/deep/deeper/file.txt" actual &&
	grep "sub/file.txt" actual
'

###########################################################################
# Section 5: -C flag path handling
###########################################################################

test_expect_success 'grit -C <path> ls-files works from outside repo' '
	grit -C repo ls-files >actual &&
	grep "file.txt" actual
'

test_expect_success 'grit -C <path> rev-parse --git-dir works' '
	result=$(grit -C repo rev-parse --git-dir) &&
	test "$result" = ".git"
'

test_expect_success 'grit -C <subdir> works from nested path' '
	grit -C repo/sub ls-files >actual &&
	cat actual >outside-sub-ls
'

test_expect_success 'grit -C with nonexistent dir fails' '
	test_must_fail grit -C nonexistent ls-files 2>err
'

###########################################################################
# Section 6: diff path handling
###########################################################################

test_expect_success 'diff shows changes to modified file' '
	cd repo &&
	echo modified >file.txt &&
	grit diff >actual &&
	grep "file.txt" actual
'

test_expect_success 'diff shows changes to nested modified file' '
	cd repo &&
	echo modified >sub/file.txt &&
	grit diff >actual &&
	grep "sub/file.txt" actual
'

test_expect_success 'diff --name-only shows all changed files' '
	cd repo &&
	grit diff --name-only >actual &&
	grep "file.txt" actual &&
	grep "sub/file.txt" actual
'

test_expect_success 'diff --name-status shows M for modified files' '
	cd repo &&
	grit diff --name-status >actual &&
	grep "M" actual | grep "file.txt"
'

test_expect_success 'diff from subdirectory still works' '
	cd repo/sub &&
	grit diff >actual &&
	cat actual >../sub-diff-output
'

test_done
