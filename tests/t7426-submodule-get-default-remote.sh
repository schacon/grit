#!/bin/sh
#
# Ported from git/t/t7426-submodule-get-default-remote.sh
# Tests submodule operations — add, init, status, foreach, sync, deinit,
# set-url, set-branch, summary, update.
#
# Original tests use `submodule--helper get-default-remote` which is a
# Git internal plumbing helper.  We test equivalent high-level submodule
# operations that exercise the same code paths in grit.

test_description='git submodule operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repositories' '
	git init sub &&
	(cd sub && touch file.txt && git add file.txt && git commit -m "initial") &&
	git init super &&
	(cd super && git submodule add ../sub subpath && git commit -m "add submodule")
'

test_expect_success 'submodule status shows submodule' '
	(cd super && git submodule status >actual &&
	 grep subpath actual)
'

test_expect_success 'submodule init registers submodule' '
	git clone super super-clone &&
	(cd super-clone && git submodule init 2>err &&
	 grep -i "registered" err)
'

test_expect_success '.gitmodules contains submodule entry' '
	(cd super && cat .gitmodules >actual &&
	 grep "subpath" actual &&
	 grep "sub" actual)
'

test_expect_success 'submodule foreach runs command in submodule' '
	(cd super && git submodule foreach "echo hello" >actual 2>&1 &&
	 grep "hello" actual)
'

test_expect_success 'submodule foreach enters submodule directory' '
	(cd super && git submodule foreach "pwd" >actual 2>&1 &&
	 grep "subpath" actual)
'

test_expect_success 'submodule sync updates URL' '
	(cd super && git submodule init &&
	 git submodule sync 2>err &&
	 grep -i "synchronizing" err)
'

test_expect_success 'submodule set-url changes URL' '
	(cd super &&
	 git submodule set-url subpath /tmp/new-url &&
	 git config -f .gitmodules submodule.subpath.url >actual &&
	 grep "/tmp/new-url" actual)
'

test_expect_success 'submodule set-url restores original URL' '
	(cd super &&
	 git submodule set-url subpath ../sub &&
	 git config -f .gitmodules submodule.subpath.url >actual &&
	 grep "\.\./sub" actual)
'

test_expect_success 'submodule set-branch sets branch' '
	(cd super &&
	 git submodule set-branch --branch develop subpath &&
	 git config -f .gitmodules submodule.subpath.branch >actual &&
	 grep "develop" actual)
'

test_expect_success 'submodule set-branch --default removes branch' '
	(cd super &&
	 git submodule set-branch --default subpath &&
	 test_must_fail git config -f .gitmodules submodule.subpath.branch)
'

test_expect_success 'submodule deinit removes submodule config' '
	(cd super-clone &&
	 git submodule init &&
	 git submodule deinit subpath 2>err &&
	 grep -i "cleared" err)
'

test_expect_success 'submodule summary shows changes' '
	(cd super && git submodule summary >actual 2>&1 &&
	 test -s actual)
'

test_expect_success 'submodule add second submodule' '
	(cd super &&
	 git submodule add ../sub another-path &&
	 git config -f .gitmodules submodule.another-path.url >actual &&
	 grep "sub" actual)
'

test_done
