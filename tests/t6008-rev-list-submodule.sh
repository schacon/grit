#!/bin/sh
#
# Ported from git/t/t6008-rev-list-submodule.sh
# Tests for 'git rev-list' involving submodules
#

test_description='git rev-list involving submodules that this repo has'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" init sub-upstream &&
	(
		cd sub-upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		: >file &&
		"$REAL_GIT" add file &&
		test_tick &&
		"$REAL_GIT" commit -m initial &&
		echo 1 >file &&
		"$REAL_GIT" add file &&
		test_tick &&
		"$REAL_GIT" commit -m second &&
		echo 2 >file &&
		"$REAL_GIT" add file &&
		test_tick &&
		"$REAL_GIT" commit -m third
	) &&

	"$REAL_GIT" init repo &&
	(
		cd repo &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" config protocol.file.allow always &&
		: >super-file &&
		"$REAL_GIT" add super-file &&
		"$REAL_GIT" -c protocol.file.allow=always submodule add "$TRASH_DIRECTORY/sub-upstream" sub &&
		"$REAL_GIT" symbolic-ref HEAD refs/heads/super &&
		test_tick &&
		"$REAL_GIT" commit -m super-initial &&
		echo 1 >super-file &&
		"$REAL_GIT" add super-file &&
		test_tick &&
		"$REAL_GIT" commit -m super-first &&
		echo 2 >super-file &&
		"$REAL_GIT" add super-file &&
		test_tick &&
		"$REAL_GIT" commit -m super-second
	)
'

test_expect_failure "Ilari s test" '
	cd repo &&
	git rev-list --objects super ^super^
'

test_done
