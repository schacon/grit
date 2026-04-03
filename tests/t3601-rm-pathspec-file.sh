#!/bin/sh

test_description='rm --pathspec-from-file'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo A >fileA.t &&
	echo B >fileB.t &&
	echo C >fileC.t &&
	echo D >fileD.t &&
	git add fileA.t fileB.t fileC.t fileD.t &&
	git commit -m "files" &&
	git tag checkpoint
'

restore_checkpoint () {
	git reset --hard checkpoint
}

test_expect_success 'rm single file' '
	restore_checkpoint &&
	git rm fileA.t &&
	test_path_is_missing fileA.t &&
	git status --porcelain >actual &&
	grep "D  fileA" actual
'

test_expect_success 'rm multiple files' '
	restore_checkpoint &&
	git rm fileB.t fileC.t &&
	test_path_is_missing fileB.t &&
	test_path_is_missing fileC.t
'

test_expect_success 'rm only touches listed files' '
	restore_checkpoint &&
	git rm fileA.t &&
	test -f fileB.t &&
	test -f fileC.t &&
	test -f fileD.t
'

test_done
