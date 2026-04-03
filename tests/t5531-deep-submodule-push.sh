#!/bin/sh
#
# Ported from git/t/t5531-deep-submodule-push.sh
# Tests for pushing with submodules
#

test_description='push with submodules'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
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
	"$REAL_GIT" init --bare pub.git &&
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo super >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub &&
		"$REAL_GIT" commit -m "add submodule" &&
		"$REAL_GIT" remote add pub "$TRASH_DIRECTORY/pub.git" &&
		"$REAL_GIT" push pub main
	)
'

test_expect_success 'push from repo with submodule' '
	cd super &&
	echo updated >file &&
	git add file &&
	git commit -m "update super" &&
	git push pub main
'

test_expect_failure 'push --recurse-submodules=check (not supported)' '
	cd super &&
	git push --recurse-submodules=check pub main
'

test_done
