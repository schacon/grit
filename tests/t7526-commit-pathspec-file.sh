#!/bin/sh
# Ported from upstream git t7526-commit-pathspec-file.sh

test_description='commit with pathspec (basic commit and log verification)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init pathspec-repo &&
	cd pathspec-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo A >fileA.t &&
	echo B >fileB.t &&
	echo C >fileC.t &&
	git add . &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'commit staged files' '
	cd pathspec-repo &&
	echo AA >fileA.t &&
	echo BB >fileB.t &&
	git add fileA.t fileB.t &&
	test_tick &&
	git commit -m "update A and B" &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'diff shows no staged changes' '
	cd pathspec-repo &&
	git diff --cached >actual &&
	test_must_be_empty actual
'

test_expect_success 'log --format shows subjects' '
	cd pathspec-repo &&
	git log --format=%s >actual &&
	grep "initial" actual &&
	grep "update A and B" actual
'

test_expect_success 'diff between commits shows changed files' '
	cd pathspec-repo &&
	git diff --name-only HEAD^ HEAD >actual &&
	grep "fileA.t" actual &&
	grep "fileB.t" actual
'

test_expect_success 'cat-file verifies content' '
	cd pathspec-repo &&
	git cat-file -p HEAD:fileA.t >actual &&
	echo "AA" >expected &&
	test_cmp expected actual
'

test_done
