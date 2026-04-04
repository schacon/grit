#!/bin/sh
# Ported from git/t/t5548-push-porcelain.sh
# Tests git push porcelain output

test_description='Test git push porcelain output'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B
'

test_expect_success 'push porcelain new branch' '
	git init --bare dst.git &&
	git push --porcelain ./dst.git main >actual 2>&1 &&
	grep "main" actual
'

test_expect_success 'push porcelain update' '
	test_commit C &&
	git push --porcelain ./dst.git main >actual 2>&1 &&
	grep "main" actual
'

test_expect_success 'push porcelain delete' '
	git push --porcelain --delete ./dst.git main >actual 2>&1 &&
	grep "main" actual
'

test_expect_success 'push porcelain rejected non-ff' '
	git push ./dst.git main &&
	git reset --hard HEAD^ &&
	test_commit D &&
	test_must_fail git push --porcelain ./dst.git main >actual 2>&1 &&
	grep "rejected" actual || grep "non-fast-forward" actual
'

test_expect_success 'push porcelain forced update' '
	git push --porcelain --force ./dst.git main >actual 2>&1 &&
	grep "main" actual
'

test_done
