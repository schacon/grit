#!/bin/sh
#
# Ported from git/t/t6041-bisect-submodule.sh
# Tests for bisect with submodules
# Note: upstream uses lib-submodule-update.sh which is complex.
# We test basic bisect in a repo that has submodules.
#

test_description='bisect can handle submodules'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup repo with submodule' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init sub &&
	(
		cd sub &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo sub >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "sub initial"
	) &&
	git init super &&
	(
		cd super &&
		git config user.email "test@example.com" &&
		git config user.name "Test User" &&
		echo good >file &&
		git add file &&
		git commit -m "good" &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub &&
		"$REAL_GIT" commit -m "add submodule" &&
		echo "change1" >>file &&
		git add file &&
		git commit -m "change1" &&
		echo "change2" >>file &&
		git add file &&
		git commit -m "change2" &&
		echo bad >>file &&
		git add file &&
		git commit -m "bad"
	)
'

test_expect_success 'bisect start works in repo with submodule' '
	cd super &&
	git bisect start &&
	git bisect bad HEAD &&
	git bisect good HEAD~3
'

test_expect_success 'bisect reset works' '
	cd super &&
	git bisect reset
'

test_done
