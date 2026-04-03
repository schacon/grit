#!/bin/sh
#
# Ported from git/t/t7406-submodule-update.sh
# Tests for 'git submodule update'
#

test_description='git submodule update'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo super >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "super initial"
	) &&
	"$REAL_GIT" init submodule &&
	(
		cd submodule &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo sub >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "sub initial" &&
		"$REAL_GIT" tag sub-initial
	) &&
	(
		cd super &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/submodule" sub &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'submodule update --init from clone' '
	"$REAL_GIT" clone super super-clone &&
	(
		cd super-clone &&
		test_path_is_missing sub/file &&
		git submodule update --init &&
		test -f sub/file
	)
'

test_expect_success 'submodule status shows registered submodules' '
	cd super &&
	git submodule status >actual &&
	grep sub actual
'

test_expect_success 'submodule init registers URLs' '
	"$REAL_GIT" clone super super-clone3 &&
	(
		cd super-clone3 &&
		git submodule init &&
		git config submodule.sub.url >actual &&
		test -s actual
	)
'

test_expect_success 'submodule foreach after setup works' '
	cd super &&
	git submodule foreach "echo working" >actual &&
	grep working actual
'

test_done
