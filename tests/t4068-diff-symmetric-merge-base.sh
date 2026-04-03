#!/bin/sh

test_description='behavior of diff with merge scenarios'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success setup '
	git commit --allow-empty -m A &&
	echo b >b &&
	git add b &&
	git commit -m B &&
	git checkout -b br1 HEAD^ &&
	echo c >c &&
	git add c &&
	git commit -m C &&
	git tag commit-C &&
	git merge -m D main &&
	git tag commit-D &&
	git checkout main &&
	git merge -m E commit-C
'

test_expect_success 'diff between tags shows changes' '
	git diff-tree --name-only -r commit-C commit-D >output &&
	grep "b" output
'

test_expect_success 'diff-tree -p shows patch content' '
	git diff-tree -p commit-C commit-D >output &&
	grep "^+b" output
'

test_expect_success 'diff-tree with --stat' '
	git diff-tree --stat -r commit-C commit-D >output &&
	grep "b" output
'

test_done
