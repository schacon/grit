#!/bin/sh
# Ported from git/t/t5535-fetch-push-symref.sh
# Tests avoiding conflicting update through symref aliasing

test_description='avoiding conflicting update through symref aliasing'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit one &&
	git clone . src &&
	git clone src dst1 &&
	git clone src dst2 &&
	test_commit two &&
	( cd src && git pull )
'

# grit does not yet support glob refspecs (refs/remotes/*:refs/remotes/*)
test_expect_success 'push' '
	(
		cd src &&
		git push ../dst1 "refs/remotes/*:refs/remotes/*"
	) &&
	git ls-remote src "refs/remotes/*" >expect &&
	git ls-remote dst1 "refs/remotes/*" >actual &&
	test_cmp expect actual &&
	( cd src && git symbolic-ref refs/remotes/origin/HEAD ) >expect &&
	( cd dst1 && git symbolic-ref refs/remotes/origin/HEAD ) >actual &&
	test_cmp expect actual
'

# grit does not yet support glob refspecs (refs/remotes/*:refs/remotes/*)
test_expect_success 'fetch' '
	(
		cd dst2 &&
		git fetch ../src "refs/remotes/*:refs/remotes/*"
	) &&
	git ls-remote src "refs/remotes/*" >expect &&
	git ls-remote dst2 "refs/remotes/*" >actual &&
	test_cmp expect actual &&
	( cd src && git symbolic-ref refs/remotes/origin/HEAD ) >expect &&
	( cd dst2 && git symbolic-ref refs/remotes/origin/HEAD ) >actual &&
	test_cmp expect actual
'

test_done
