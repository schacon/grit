#!/bin/sh
# Ported from git/t/t5515-fetch-merge-logic.sh
# Simplified: tests basic fetch and merge logic

test_description='Merge logic in fetch'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo one >file &&
	git add file &&
	git commit -m one &&
	git branch side &&
	echo two >file &&
	git add file &&
	git commit -m two
'

test_expect_success 'fetch updates remote-tracking branches' '
	git clone . fetcher &&
	echo three >file &&
	git add file &&
	git commit -m three &&
	(
		cd fetcher &&
		git fetch origin &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)" &&
		test "$(git rev-parse origin/side)" = "$(cd .. && git rev-parse side)"
	)
'

test_expect_success 'fetch --prune removes deleted remote branch' '
	git branch to-prune &&
	(
		cd fetcher &&
		git fetch origin &&
		git rev-parse origin/to-prune
	) &&
	git branch -D to-prune &&
	(
		cd fetcher &&
		git fetch --prune origin &&
		test_must_fail git rev-parse origin/to-prune
	)
'

test_done
