#!/bin/sh
# Ported from git/t/t5582-fetch-negative-refspec.sh
# Tests "git fetch" with negative refspecs

test_description='"git fetch" with negative refspecs'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo >file original &&
	git add file &&
	git commit -a -m original
'

test_expect_success 'clone and setup child repos' '
	git clone . one &&
	(
		cd one &&
		echo >file updated by one &&
		git commit -a -m "updated by one" &&
		git checkout -b alternate &&
		echo >file updated again by one &&
		git commit -a -m "updated by one again" &&
		git checkout main
	) &&
	git clone . two &&
	(
		cd two &&
		git config branch.main.remote one &&
		git config remote.one.url ../one/.git/ &&
		git config remote.one.fetch "+refs/heads/*:refs/remotes/one/*" &&
		git config --add remote.one.fetch "^refs/heads/alternate"
	) &&
	git clone . three
'

# grit does not support negative refspecs (^refs/...)
test_expect_failure 'fetch with negative refspec in config' '
	(
		cd two &&
		test_must_fail git rev-parse --verify refs/remotes/one/alternate &&
		git fetch one &&
		test_must_fail git rev-parse --verify refs/remotes/one/alternate &&
		git rev-parse --verify refs/remotes/one/main
	)
'

# grit does not support negative refspec on command line
test_expect_failure 'fetch with negative refspec on commandline' '
	echo >file updated by origin again &&
	git commit -a -m "updated by origin again" &&
	(
		cd three &&
		git fetch ../one/.git "refs/heads/*:refs/remotes/one/*" "^refs/heads/main" &&
		alternate_in_one=$(cd ../one && git rev-parse refs/heads/alternate) &&
		echo $alternate_in_one >expect &&
		cut -f -1 .git/FETCH_HEAD >actual &&
		test_cmp expect actual
	)
'

# grit rejects glob refspecs, so the fetch fails regardless
test_expect_success 'fetch with negative sha1 refspec fails' '
	(
		cd three &&
		main_in_one=$(cd ../one && git rev-parse refs/heads/main) &&
		test_must_fail git fetch ../one/.git "refs/heads/*:refs/remotes/one/*" "^$main_in_one"
	)
'

test_expect_success 'basic fetch without negative refspec works' '
	(
		cd two &&
		git fetch ../one/.git refs/heads/main:refs/remotes/one/main &&
		git rev-parse --verify refs/remotes/one/main
	)
'

test_done
