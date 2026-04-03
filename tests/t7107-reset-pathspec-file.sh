#!/bin/sh
# Adapted from git/t/t7107-reset-pathspec-file.sh
# Tests reset with path arguments

test_description='reset with paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init reset-paths &&
	cd reset-paths &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo A >fileA.t &&
	echo B >fileB.t &&
	echo C >fileC.t &&
	echo D >fileD.t &&
	git add . &&
	git commit -m "Commit" &&
	git tag checkpoint
'

test_expect_success 'reset individual file from index' '
	cd reset-paths &&
	git reset --hard checkpoint &&
	git rm fileA.t &&
	git reset -- fileA.t &&
	git status --porcelain >actual &&
	test_grep "fileA.t" actual
'

test_expect_success 'reset only touches specified paths' '
	cd reset-paths &&
	git reset --hard checkpoint &&
	git rm fileA.t fileB.t fileC.t fileD.t &&
	git reset -- fileB.t fileC.t &&
	git status --porcelain >actual &&
	test_grep "D  fileA.t" actual &&
	test_grep "D  fileD.t" actual
'

test_expect_success 'reset to specific commit' '
	cd reset-paths &&
	git reset --hard checkpoint &&
	echo "new" >>fileA.t &&
	git add fileA.t &&
	git commit -m "modify A" &&
	git reset checkpoint -- fileA.t &&
	git status --porcelain >actual &&
	test_grep "fileA.t" actual
'

test_done
