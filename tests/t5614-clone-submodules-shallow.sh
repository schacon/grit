#!/bin/sh
#
# Ported from git/t/t5614-clone-submodules-shallow.sh
# Tests for cloning submodules with --shallow-submodules
# Note: grit clone does not support --recurse-submodules
#

test_description='clone with --shallow-submodules'

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
		"$REAL_GIT" commit -m "sub initial" &&
		echo sub2 >>file &&
		"$REAL_GIT" commit -am "sub second"
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

test_expect_failure 'clone with --recurse-submodules' '
	git clone --recurse-submodules super clone1 &&
	test -f clone1/sub/file
'

test_expect_failure 'basic clone of super repo (grit fails on submodule gitlink during checkout)' '
	git clone super clone2 &&
	test -f clone2/file
'

test_done
