#!/bin/sh
# Ported from git/t/t5402-post-merge-hook.sh
# Simplified: tests merge via pull (hooks not yet supported)

test_description='Test merge functionality via pull'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo Data for commit0. >a &&
	git add a &&
	git commit -m setup &&
	git clone . clone1
'

test_expect_success 'pull up-to-date does nothing' '
	(
		cd clone1 &&
		old_head=$(git rev-parse HEAD) &&
		git pull &&
		new_head=$(git rev-parse HEAD) &&
		test "$old_head" = "$new_head"
	)
'

test_expect_success 'pull with new commit fast-forwards' '
	echo Changed data >a &&
	git add a &&
	git commit -m modify &&
	(
		cd clone1 &&
		git pull &&
		test "$(git rev-parse HEAD)" = "$(cd .. && git rev-parse main)"
	)
'

test_done
