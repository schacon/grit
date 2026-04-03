#!/bin/sh
#
# Ported from git/t/t6134-pathspec-in-submodule.sh
# Tests for pathspec handling in submodules
#

test_description='test case exclude pathspec'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup a submodule' '
	git init main-repo &&
	cd main-repo &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	test_create_repo pretzel &&
	: >pretzel/a &&
	"$REAL_GIT" -C pretzel add a &&
	"$REAL_GIT" -C pretzel commit -m "add a file" -- a &&
	"$REAL_GIT" -c protocol.file.allow=always submodule add ./pretzel sub &&
	"$REAL_GIT" commit -a -m "add submodule" &&
	"$REAL_GIT" submodule deinit --all
'

test_expect_failure 'error message for path inside submodule' '
	cd main-repo &&
	echo a >sub/a &&
	test_must_fail git add sub/a 2>actual &&
	grep "in submodule" actual
'

test_done
