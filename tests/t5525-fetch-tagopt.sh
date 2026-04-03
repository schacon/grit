#!/bin/sh
# Ported from git/t/t5525-fetch-tagopt.sh

test_description='tagopt variable affects "git fetch" and is overridden by commandline.'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

setup_clone () {
	git clone --bare . $1 &&
	git remote add remote_$1 $1 &&
	(cd $1 &&
	git tag tag_$1 &&
	git branch branch_$1)
}

test_expect_success setup '
	git init -q &&
	echo test >file &&
	git add file &&
	git commit -m test &&
	setup_clone one &&
	setup_clone two
'

test_expect_success 'fetch --no-tags does not get tag' '
	git fetch --no-tags remote_two &&
	test_must_fail git show-ref tag_two &&
	git show-ref remote_two/branch_two
'

test_expect_success 'fetch --tags gets tag' '
	git fetch --tags remote_one &&
	git show-ref tag_one &&
	git show-ref remote_one/branch_one
'

test_done
