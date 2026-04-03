#!/bin/sh
# Ported from git/t/t5528-push-default.sh

test_description='check various push.default settings'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup bare remotes' '
	git init -q &&
	git init --bare repo1 &&
	git remote add parent1 repo1 &&
	echo one >one.t &&
	git add one.t &&
	git commit -m one &&
	git tag one &&
	git push parent1 main
'

test_expect_success 'push with configured upstream' '
	git config branch.main.remote parent1 &&
	git config branch.main.merge refs/heads/main &&
	echo two >two.t &&
	git add two.t &&
	git commit -m two &&
	git push &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=repo1 rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_expect_success 'push with explicit remote' '
	echo three >three.t &&
	git add three.t &&
	git commit -m three &&
	git push parent1 &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=repo1 rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_expect_success 'push force with -f flag' '
	git reset --hard HEAD^ &&
	git push -f parent1 main &&
	local_head=$(git rev-parse main) &&
	remote_head=$(git --git-dir=repo1 rev-parse main) &&
	test "$local_head" = "$remote_head"
'

test_done
