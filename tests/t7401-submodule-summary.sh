#!/bin/sh
#
# Ported from git/t/t7401-submodule-summary.sh
# Tests for 'git submodule summary'
# Note: grit does not implement 'submodule summary' yet
#

test_description='Summary support for submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init sm1 &&
	(
		cd sm1 &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo file >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "add file"
	) &&
	git init main-repo &&
	(
		cd main-repo &&
		git config user.email "test@example.com" &&
		git config user.name "Test User" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	)
'

test_expect_success 'submodule summary basic usage' '
	cd main-repo &&
	"$REAL_GIT" -c protocol.file.allow=always submodule add ../sm1 sm1 &&
	"$REAL_GIT" commit -m "add submodule" &&
	git submodule summary
'

test_done
