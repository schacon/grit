#!/bin/sh

test_description='repack with alternate objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup base repo' '
	$REAL_GIT init repack-alt-base &&
	cd repack-alt-base &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo content >file &&
	$REAL_GIT add file &&
	$REAL_GIT commit -m "base"
'

test_expect_success 'clone and verify with grit' '
	$REAL_GIT clone repack-alt-base repack-alt-clone &&
	cd repack-alt-clone &&
	git cat-file -t HEAD >output &&
	echo commit >expect &&
	test_cmp expect output
'

test_expect_success 'grit reads cloned repo log' '
	cd repack-alt-clone &&
	git log --oneline >output &&
	test -s output
'

test_done
