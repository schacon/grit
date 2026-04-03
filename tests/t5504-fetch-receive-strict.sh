#!/bin/sh
# Ported from git/t/t5504-fetch-receive-strict.sh
# Simplified: tests basic fetch/push to repos

test_description='fetch/receive basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo hello >greetings &&
	git add greetings &&
	git commit -m greetings
'

test_expect_success 'clone and verify' '
	git clone . dst &&
	(
		cd dst &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_expect_success 'push to bare repo' '
	git init --bare dst-bare.git &&
	git send-pack ./dst-bare.git main &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=dst-bare.git rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_done
