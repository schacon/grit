#!/bin/sh
# Ported from git/t/t5510-fetch.sh
# Basic fetch tests

test_description='git fetch basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo original >file &&
	git add file &&
	git commit -m original
'

test_expect_success 'clone and setup child repos' '
	git clone . one &&
	(
		cd one &&
		git config user.email "test@example.com" &&
		git config user.name "Test User" &&
		echo "updated by one" >file &&
		git commit -a -m "updated by one"
	) &&
	git clone . two &&
	(
		cd two &&
		git config user.email "test@example.com" &&
		git config user.name "Test User"
	)
'

test_expect_success 'fetch from another clone' '
	(
		cd two &&
		git remote add one ../one &&
		git fetch one &&
		git rev-parse --verify refs/remotes/one/main
	)
'

test_expect_success 'fetch updates remote tracking branches' '
	echo "updated by origin" >file &&
	git commit -a -m "updated by origin" &&
	(
		cd one &&
		git fetch origin &&
		mine=$(git rev-parse refs/remotes/origin/main) &&
		his=$(cd .. && git rev-parse refs/heads/main) &&
		test "z$mine" = "z$his"
	)
'

test_expect_success 'fetch --all' '
	(
		cd two &&
		git fetch --all
	)
'

test_expect_success 'fetch --prune removes stale remote tracking branches' '
	(
		cd one &&
		git checkout -b stale-branch &&
		echo stale >stale &&
		git add stale &&
		git commit -m stale &&
		git checkout main
	) &&
	(
		cd two &&
		git fetch one &&
		git rev-parse --verify refs/remotes/one/stale-branch
	) &&
	(
		cd one &&
		git branch -D stale-branch
	) &&
	(
		cd two &&
		git fetch --prune one &&
		test_must_fail git rev-parse --verify refs/remotes/one/stale-branch
	)
'

test_expect_success 'fetch --tags brings tags' '
	git tag test-tag &&
	(
		cd one &&
		git fetch --tags origin &&
		git rev-parse --verify refs/tags/test-tag
	)
'

test_expect_success 'fetch --no-tags does not bring tags' '
	git tag another-tag &&
	(
		cd two &&
		git fetch --no-tags origin &&
		test_must_fail git rev-parse --verify refs/tags/another-tag
	)
'

test_expect_success 'fetch -q is quiet' '
	(
		cd one &&
		git fetch -q origin 2>err &&
		test_must_be_empty err
	)
'

test_done
