#!/bin/sh
#
# Ported from git/t/t7421-submodule-summary-add.sh
# Tests for submodule summary when added via submodule add
# Note: grit does not implement 'submodule summary' yet
#

test_description='Summary support for submodules, adding them using git submodule add'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init sm &&
	(
		cd sm &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo file-content >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m "add file" &&
		"$REAL_GIT" tag file-tag
	) &&
	git init super &&
	(
		cd super &&
		git config user.email "test@example.com" &&
		git config user.name "Test User" &&
		echo content >initial &&
		git add initial &&
		git commit -m initial
	)
'

test_expect_success 'submodule summary output for initialized submodule' '
	cd super &&
	"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sm" my-subm &&
	"$REAL_GIT" commit -m "add submodule" &&
	git submodule summary >actual &&
	test -s actual
'

test_done
