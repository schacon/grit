#!/bin/sh
#
# Ported from git/t/t7408-submodule-reference.sh
# Tests for submodule --reference
# Note: grit clone does not support --recurse-submodules or --reference for submodules
#

test_description='Tests for git submodule --reference'

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
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'clone with --recurse-submodules and --reference' '
	git clone --recurse-submodules --reference super super clone-ref &&
	test -f clone-ref/sub/file
'

test_done
