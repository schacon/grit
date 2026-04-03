#!/bin/sh
#
# Ported from git/t/t7420-submodule-set-url.sh
# Tests for 'git submodule set-url'
# Note: grit does not implement 'submodule set-url' yet
#

test_description='Test submodule set-url subcommand'

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

test_expect_success 'submodule set-url' '
	cd super &&
	git submodule set-url sub /some/new/url
'

test_done
