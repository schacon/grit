#!/bin/sh
#
# Ported from git/t/t7417-submodule-path-url.sh
# Tests for handling of .gitmodule path with dash
#

test_description='check handling of .gitmodule path with dash'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always
'

test_expect_success 'create submodule with dash in path' '
	"$REAL_GIT" init upstream &&
	(
		cd upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" commit --allow-empty -m base
	) &&
	"$REAL_GIT" init main-repo &&
	(
		cd main-repo &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo content >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/upstream" sub &&
		"$REAL_GIT" mv sub ./-sub &&
		"$REAL_GIT" commit -m submodule
	)
'

test_expect_success 'submodule status shows submodule with dash path' '
	cd main-repo &&
	git submodule status >actual &&
	test -s actual
'

test_done
