#!/bin/sh

test_description='messages from rebase operation'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit O fileO &&
	test_commit X fileX &&
	test_commit A fileA &&
	test_commit B fileB &&

	git checkout -b topic O &&
	test_commit Z fileZ &&
	git tag start
'

test_expect_success 'rebase onto main' '
	git checkout topic &&
	git reset --hard start &&
	git rebase main
'

test_expect_success 'topic is now on top of main' '
	git rev-parse main >expect &&
	git rev-parse HEAD~1 >actual &&
	test_cmp expect actual
'

test_expect_success 'rebase preserves commit message' '
	git cat-file commit HEAD | sed -e "1,/^\$/d" >actual &&
	echo Z >expect &&
	test_cmp expect actual
'

test_done
