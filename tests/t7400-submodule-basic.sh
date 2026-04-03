#!/bin/sh
# Adapted from git/t/t7400-submodule-basic.sh
# Tests basic submodule operations

test_description='basic submodule operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup: create submodule repo' '
	mkdir submod-src &&
	cd submod-src &&
	"$REAL_GIT" init &&
	"$REAL_GIT" config user.name "Test" &&
	"$REAL_GIT" config user.email "t@t.com" &&
	echo "sub content" >file &&
	"$REAL_GIT" add file &&
	"$REAL_GIT" commit -m "sub initial" &&
	cd ..
'

test_expect_success 'setup: create super repo with submodule' '
	mkdir super &&
	cd super &&
	"$REAL_GIT" init &&
	"$REAL_GIT" config user.name "Test" &&
	"$REAL_GIT" config user.email "t@t.com" &&
	"$REAL_GIT" config protocol.file.allow always &&
	echo "super" >super.txt &&
	"$REAL_GIT" add super.txt &&
	"$REAL_GIT" commit -m "super initial" &&
	"$REAL_GIT" -c protocol.file.allow=always submodule add "$TRASH_DIRECTORY/submod-src" submod &&
	test_path_is_file .gitmodules &&
	test_path_is_dir submod &&
	test_path_is_file submod/file &&
	"$REAL_GIT" commit -m "add submodule"
'

test_expect_success 'grit submodule status works' '
	cd super &&
	git submodule status >output &&
	test_grep "submod" output
'

test_expect_success 'grit submodule foreach works' '
	cd super &&
	git submodule foreach "echo working" >output 2>&1 &&
	test_grep "working" output
'

test_done
