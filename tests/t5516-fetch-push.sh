#!/bin/sh
# Ported from git/t/t5516-fetch-push.sh

test_description='Basic fetch/push functionality'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	echo more >file &&
	git add file &&
	git commit -m second
'

test_expect_success 'clone and push back' '
	git clone . child &&
	(
		cd child &&
		echo third >file &&
		git add file &&
		git commit -m third &&
		git push origin main
	) &&
	child_head=$(cd child && git rev-parse main) &&
	local_head=$(git rev-parse main) &&
	test "$child_head" = "$local_head"
'

test_expect_success 'send-pack to bare repo' '
	git init --bare bare.git &&
	git send-pack ./bare.git main &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=bare.git rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_expect_success 'send-pack with refspec' '
	git init --bare target.git &&
	git send-pack ./target.git main:refs/heads/other &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=target.git rev-parse refs/heads/other) &&
	test "$local_head" = "$remote_head"
'

test_expect_success 'send-pack force overwrites non-ff' '
	git init --bare ff-target.git &&
	git send-pack ./ff-target.git main &&
	git reset --hard HEAD^ &&
	echo diverged >file &&
	git add file &&
	git commit -m diverged &&
	test_must_fail git send-pack ./ff-target.git main &&
	git send-pack --force ./ff-target.git main &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=ff-target.git rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_expect_success 'push --tags pushes tags to remote' '
	git clone . tag-child &&
	(
		cd tag-child &&
		git tag my-test-tag &&
		git push --tags origin &&
		git show-ref my-test-tag
	) &&
	git show-ref my-test-tag
'

test_expect_success 'push --delete removes remote branch' '
	git clone . del-child &&
	git branch to-delete &&
	(
		cd del-child &&
		git fetch origin &&
		git push --delete origin to-delete
	) &&
	test_must_fail git show-ref refs/heads/to-delete
'

test_done
