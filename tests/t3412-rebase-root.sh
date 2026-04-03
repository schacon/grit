#!/bin/sh

test_description='git rebase --onto tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	git checkout -b side A &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rebase --onto works' '
	git checkout side &&
	git rebase --onto C A &&
	git rev-parse C >expect &&
	git rev-parse HEAD~2 >actual &&
	test_cmp expect actual
'

test_expect_success 'rebased commits preserved' '
	git cat-file commit HEAD | grep "E" &&
	git cat-file commit HEAD^ | grep "D"
'

test_done
