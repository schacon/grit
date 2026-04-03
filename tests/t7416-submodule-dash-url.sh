#!/bin/sh
#
# Ported from git/t/t7416-submodule-dash-url.sh
# Tests for handling of disallowed .gitmodule URLs
#

test_description='check handling of disallowed .gitmodule urls'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always
'

test_expect_success 'create submodule with protected dash in url' '
	"$REAL_GIT" init upstream &&
	(
		cd upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" commit --allow-empty -m base
	) &&
	mv upstream ./-upstream &&
	"$REAL_GIT" init main-repo &&
	(
		cd main-repo &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo content >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/-upstream" sub &&
		"$REAL_GIT" add sub .gitmodules &&
		"$REAL_GIT" commit -m submodule
	)
'

test_expect_success 'clone can recurse submodule with dash url' '
	git clone --recurse-submodules main-repo dst &&
	test -d dst/sub
'

test_expect_success 'submodule status works on repo with dash url' '
	cd main-repo &&
	git submodule status >actual &&
	grep sub actual
'

test_done
