#!/bin/sh
#
# Ported from git/t/t5617-clone-submodules-remote.sh
# Tests for clone with --recurse-submodules --remote-submodules
# Note: grit clone does not support --recurse-submodules yet
#

test_description='clone with --recurse-submodules --remote-submodules'

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
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo super >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "super initial" &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'clone super repo - grit checkout fails on submodule gitlinks' '
	git clone super clone1 &&
	test -f clone1/file &&
	test_path_is_missing clone1/sub/file
'

test_expect_success 'clone with --recurse-submodules' '
	git clone --recurse-submodules super clone2 &&
	test -f clone2/sub/file
'

test_expect_success 'clone with --recurse-submodules --remote-submodules' '
	git clone --recurse-submodules --remote-submodules super clone3 &&
	test -f clone3/sub/file
'

test_done
