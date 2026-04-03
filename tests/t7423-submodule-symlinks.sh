#!/bin/sh
#
# Ported from git/t/t7423-submodule-symlinks.sh
# Tests that submodule operations do not follow symlinks
#

test_description='check that submodule operations do not follow symlinks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'prepare' '
	"$REAL_GIT" init upstream &&
	(
		cd upstream &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo upstream >submodule_file &&
		"$REAL_GIT" add submodule_file &&
		"$REAL_GIT" commit -m upstream
	) &&
	"$REAL_GIT" init main-repo &&
	(
		cd main-repo &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" config protocol.file.allow always &&
		echo initial >initial.t &&
		"$REAL_GIT" add initial.t &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" tag initial &&
		"$REAL_GIT" -c protocol.file.allow=always submodule add ../upstream a/sm &&
		"$REAL_GIT" commit -m submodule
	)
'

test_expect_success SYMLINKS 'git submodule update must not create submodule behind symlink' '
	cd main-repo &&
	rm -rf a b &&
	mkdir b &&
	ln -s b a &&
	test_path_is_missing b/sm &&
	test_must_fail git submodule update &&
	test_path_is_missing b/sm
'

test_done
