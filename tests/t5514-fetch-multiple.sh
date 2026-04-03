#!/bin/sh
# Ported from git/t/t5514-fetch-multiple.sh

test_description='fetch --all works correctly'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

setup_repository () {
	mkdir "$1" && (
	cd "$1" &&
	git init &&
	>file &&
	git add file &&
	test_tick &&
	git commit -m "Initial" &&
	git checkout -b side &&
	>elif &&
	git add elif &&
	test_tick &&
	git commit -m "Second" &&
	git checkout main
	)
}

test_expect_success setup '
	git init -q &&
	setup_repository one &&
	setup_repository two
'

test_expect_success 'git fetch --all fetches from all remotes' '
	git clone one test &&
	(cd test &&
	 git remote add one ../one &&
	 git remote add two ../two &&
	 git fetch --all &&
	 git rev-parse one/main &&
	 git rev-parse two/main
	)
'

test_expect_success 'git fetch --all should fail if a remote has errors' '
	git clone one test2 &&
	(cd test2 &&
	 git remote add bad ../non-existing &&
	 git remote add two ../two &&
	 test_must_fail git fetch --all
	)
'

test_expect_success 'git fetch --all --no-tags' '
	git clone one test5 &&
	git clone test5 test6 &&
	(cd test5 && git tag test-tag) &&
	(
		cd test6 &&
		git fetch --all --no-tags &&
		git tag >output
	) &&
	test_must_be_empty test6/output
'

test_expect_success 'git fetch --all --tags' '
	git clone one test7 &&
	git clone test7 test8 &&
	(
		cd test7 &&
		echo content >newfile &&
		git add newfile &&
		git commit -m "new" &&
		git tag test-tag &&
		git reset --hard HEAD^
	) &&
	(
		cd test8 &&
		git fetch --all --tags &&
		git tag >output &&
		grep test-tag output
	)
'

test_done
