#!/bin/sh
# Ported from git/t/t5523-push-upstream.sh

test_description='push with --set-upstream'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup bare parent' '
	git init -q &&
	git init --bare parent &&
	git remote add upstream parent
'

test_expect_success 'setup local commit' '
	echo content >file &&
	git add file &&
	git commit -m one
'

check_config() {
	(echo $2; echo $3) >expect.$1
	(git config branch.$1.remote
	 git config branch.$1.merge) >actual.$1
	test_cmp expect.$1 actual.$1
}

test_expect_success 'push -u main:main sets upstream' '
	git push -u upstream main:main &&
	check_config main upstream refs/heads/main
'

test_expect_success 'push to upstream updates refs' '
	echo more >file &&
	git add file &&
	git commit -m two &&
	git push upstream main &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=parent rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_done
