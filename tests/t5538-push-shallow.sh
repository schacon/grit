#!/bin/sh
# Ported from git/t/t5538-push-shallow.sh
# Tests push from/to a shallow clone

test_description='push from/to a shallow clone'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit one &&
	test_commit two &&
	test_commit three &&
	test_commit four
'

test_expect_success 'push from shallow clone' '
	git clone --depth 2 . shallow-clone &&
	(
		cd shallow-clone &&
		test_commit five &&
		git push origin main
	) &&
	git log --oneline main >actual &&
	test_line_count = 5 actual
'

test_expect_success 'push to shallow clone' '
	git clone --depth 2 . shallow-dest &&
	(
		cd shallow-dest &&
		git config receive.denyCurrentBranch warn
	) &&
	test_commit six &&
	git push ./shallow-dest main
'

test_expect_success 'push new branch from shallow clone' '
	(
		cd shallow-clone &&
		git checkout -b new-branch &&
		test_commit on-new-branch &&
		git push origin new-branch
	) &&
	git rev-parse new-branch >expect &&
	(cd shallow-clone && git rev-parse new-branch) >actual &&
	test_cmp expect actual
'

test_done
