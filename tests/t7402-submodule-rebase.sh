#!/bin/sh
#
# Ported from git/t/t7402-submodule-rebase.sh
# Tests for rebasing with submodules
#

test_description='Test rebasing, stashing, etc. with submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	git init repo &&
	cd repo &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo file >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	"$REAL_GIT" clone . submodule &&
	git add submodule &&
	test_tick &&
	git commit -m submodule &&
	echo "second line" >>file &&
	(cd submodule && "$REAL_GIT" pull) &&
	test_tick &&
	git commit -m file-and-submodule -a &&
	git branch added-submodule
'

test_expect_success 'rebase with a dirty submodule' '
	cd repo &&
	(cd submodule &&
	 echo "3rd line" >>file &&
	 test_tick &&
	 "$REAL_GIT" commit -m fork -a) &&
	echo unrelated >>file2 &&
	git add file2 &&
	test_tick &&
	git commit -m unrelated &&
	echo other_line >>file &&
	git add file &&
	test_tick &&
	git commit -m update &&
	CURRENT=$(cd submodule && git rev-parse HEAD) &&
	EXPECTED=$(git rev-parse HEAD~2:submodule) &&
	git rebase --onto HEAD~2 HEAD^ &&
	STORED=$(git rev-parse HEAD:submodule) &&
	test $EXPECTED = $STORED &&
	test $CURRENT = $(cd submodule && git rev-parse HEAD)
'

test_done
