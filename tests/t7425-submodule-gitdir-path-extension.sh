#!/bin/sh
#
# Ported from git/t/t7425-submodule-gitdir-path-extension.sh
# Tests for submodulePathConfig extension
# Note: grit doesn't implement this extension yet
#

test_description='submodulePathConfig extension works as expected'

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
	"$REAL_GIT" init -b main super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo super >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "super initial" &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub-path &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'submodule status works for basic setup' '
	cd super &&
	git submodule status >actual &&
	grep sub-path actual
'

test_expect_success 'submodule foreach works for basic setup' '
	cd super &&
	git submodule foreach "echo working" >actual &&
	grep working actual
'

test_done
