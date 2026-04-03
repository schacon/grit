#!/bin/sh
# Ported from git/t/t5506-remote-groups.sh
# Simplified: tests basic remote management

test_description='git remote handling'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	mkdir one && (cd one && git init && echo file >file && git add file && git commit -m one) &&
	mkdir two && (cd two && git init && echo file >file && git add file && git commit -m two) &&
	git remote add one one &&
	git remote add two two
'

test_expect_success 'remote -v lists remotes' '
	git remote -v >output &&
	grep "one" output &&
	grep "two" output
'

test_expect_success 'fetch individual remote' '
	git fetch one &&
	git rev-parse one/main
'

test_expect_success 'fetch --all fetches all remotes' '
	git fetch --all &&
	git rev-parse one/main &&
	git rev-parse two/main
'

test_expect_success 'remote rename works' '
	git remote rename one first &&
	git remote -v >output &&
	grep "first" output &&
	! grep "^one" output
'

test_expect_success 'remote remove works' '
	git remote remove two &&
	git remote -v >output &&
	! grep "^two" output
'

test_done
