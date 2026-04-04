#!/bin/sh

test_description='reftables are compatible with JGit'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# JGit compatibility tests require a JGit binary, which is not
# bundled with grit.  Verify that grit's reftable backend can at
# least create a valid reftable repository, then skip the JGit
# cross-tool checks.

test_expect_success 'CGit reftable repository is self-consistent' '
	git init --ref-format=reftable repo-rt &&
	(
		cd repo-rt &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	) &&
	git -C repo-rt show-ref >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'reftable repo round-trips through grit' '
	git -C repo-rt log --oneline >actual &&
	grep initial actual
'

test_done
