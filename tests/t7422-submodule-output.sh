#!/bin/sh
#
# Ported from git/t/t7422-submodule-output.sh
# Tests for submodule --cached, --quiet etc. output
# Note: many features not yet in grit
#

test_description='submodule --cached, --quiet etc. output'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	git init main-repo &&
	(
		cd main-repo &&
		git config user.email "test@example.com" &&
		git config user.name "Test User" &&
		echo A >A.t &&
		git add A.t &&
		git commit -m A &&
		git tag A &&
		echo B >B.t &&
		git add B.t &&
		git commit -m B &&
		git tag B
	) &&
	"$REAL_GIT" clone main-repo sub-src &&
	(
		cd main-repo &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub-src" S &&
		"$REAL_GIT" commit -m "add S"
	)
'

test_expect_success 'submodule status basic output' '
	cd main-repo &&
	git submodule status >actual &&
	grep S actual
'

test_expect_success 'submodule foreach basic output' '
	cd main-repo &&
	git submodule foreach "echo ok" >actual &&
	grep ok actual
'

test_done
