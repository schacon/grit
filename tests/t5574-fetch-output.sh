#!/bin/sh
# Ported from git/t/t5574-fetch-output.sh
# Tests git fetch output format

test_description='git fetch output format'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit initial
'

test_expect_success 'fetch shows updated refs' '
	git clone . fetch-output &&
	test_commit second &&
	(
		cd fetch-output &&
		git fetch origin 2>stderr &&
		grep "main" stderr
	)
'

test_expect_success 'fetch shows new tags' '
	git tag new-tag HEAD &&
	(
		cd fetch-output &&
		git fetch origin 2>stderr &&
		grep "new-tag" stderr
	)
'

test_expect_success 'fetch shows new branches' '
	git checkout -b new-branch &&
	test_commit on-new-branch &&
	git checkout main &&
	(
		cd fetch-output &&
		git fetch origin 2>stderr &&
		grep "new-branch" stderr
	)
'

# grit does not support fetch.output config
test_expect_failure 'fetch with invalid output format configuration' '
	git clone . clone-for-config &&
	(
		cd clone-for-config &&
		test_must_fail git -c fetch.output fetch origin 2>actual.err &&
		grep "fetch.output" actual.err
	)
'

# grit does not support --porcelain for fetch
test_expect_success 'fetch porcelain output' '
	git clone . porcelain-clone &&
	test_commit for-porcelain &&
	(
		cd porcelain-clone &&
		git fetch --porcelain origin >actual &&
		grep "refs/remotes/origin/main" actual
	)
'

# grit does not support --no-show-forced-updates
test_expect_success '--no-show-forced-updates' '
	mkdir forced-updates &&
	(
		cd forced-updates &&
		git init &&
		test_commit 1 &&
		test_commit 2
	) &&
	git clone forced-updates no-forced-clone &&
	git -C forced-updates reset --hard HEAD~1 &&
	(
		cd no-forced-clone &&
		git fetch --no-show-forced-updates origin 2>output &&
		test_grep ! "(forced update)" output
	)
'

test_done
