#!/bin/sh
#
# Ported from git/t/t7403-submodule-sync.sh
# Tests for 'git submodule sync'
# Note: grit does not implement 'submodule sync' yet
#

test_description='git submodule sync'

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
		echo file >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m upstream
	) &&
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" submodule add ../sub sub &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'submodule sync updates url' '
	cd super &&
	git submodule sync
'

test_done
