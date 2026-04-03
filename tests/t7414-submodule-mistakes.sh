#!/bin/sh
#
# Ported from git/t/t7414-submodule-mistakes.sh
# Tests for handling of common mistakes people may make with submodules
#

test_description='handling of common mistakes people may make with submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	git init main-repo &&
	cd main-repo &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'create embedded repository' '
	cd main-repo &&
	"$REAL_GIT" init embed &&
	(
		cd embed &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo one >one.t &&
		"$REAL_GIT" add one.t &&
		"$REAL_GIT" commit -m one
	)
'

test_expect_success 'git-add on embedded repository warns' '
	cd main-repo &&
	git add embed 2>stderr &&
	test_grep warning stderr &&
	git rm --cached -f embed
'

test_expect_success '--no-warn-embedded-repo suppresses warning' '
	cd main-repo &&
	git add --no-warn-embedded-repo embed 2>stderr &&
	test_grep ! warning stderr &&
	git rm --cached -f embed
'

test_expect_success 'no warning when updating entry' '
	cd main-repo &&
	git add embed &&
	(cd embed && "$REAL_GIT" commit --allow-empty -m two) &&
	git add embed 2>stderr &&
	test_grep ! warning stderr &&
	git rm --cached -f embed
'

test_expect_failure 'submodule add does not warn' '
	cd main-repo &&
	git -c protocol.file.allow=always \
		submodule add ./embed submodule 2>stderr &&
	test_grep ! warning stderr &&
	git rm -rf submodule .gitmodules
'

test_done
