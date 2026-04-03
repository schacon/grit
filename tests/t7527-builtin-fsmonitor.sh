#!/bin/sh
# Ported from upstream git t7527-builtin-fsmonitor.sh
# fsmonitor not available, test status/add/diff that fsmonitor would accelerate

test_description='builtin fsmonitor scenarios (status/diff verification)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init fsmon-builtin &&
	cd fsmon-builtin &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file1 &&
	echo base >file2 &&
	echo base >file3 &&
	mkdir subdir &&
	echo nested >subdir/file4 &&
	git add . &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'status shows branch after commit' '
	cd fsmon-builtin &&
	git status >actual &&
	grep "On branch" actual
'

test_expect_success 'status detects modification' '
	cd fsmon-builtin &&
	echo changed >file1 &&
	git status --porcelain >actual &&
	grep "file1" actual
'

test_expect_success 'diff detects modification' '
	cd fsmon-builtin &&
	git diff --name-only >actual &&
	grep "file1" actual
'

test_expect_success 'status detects new files' '
	cd fsmon-builtin &&
	echo new >newfile &&
	git status --porcelain >actual &&
	grep "newfile" actual
'

test_expect_success 'status detects deleted files' '
	cd fsmon-builtin &&
	rm file2 &&
	git status --porcelain >actual &&
	grep "file2" actual
'

test_expect_success 'add and commit changed files' '
	cd fsmon-builtin &&
	git add file1 newfile &&
	git rm file2 &&
	test_tick &&
	git commit -m "changes" &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'status in subdirectory' '
	cd fsmon-builtin &&
	echo changed >subdir/file4 &&
	git status --porcelain >actual &&
	grep "subdir/file4" actual
'

test_expect_success 'log shows commits' '
	cd fsmon-builtin &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_done
