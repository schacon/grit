#!/bin/sh

test_description='Test merge state after operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init merge-state &&
	cd merge-state &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&

	echo base >base &&
	$REAL_GIT add base &&
	$REAL_GIT commit -m "Initial" &&

	for b in branch1 branch2 branch3; do
		$REAL_GIT checkout -b $b master &&
		echo "change on $b" >base &&
		$REAL_GIT add base &&
		$REAL_GIT commit -m "Change on $b" || return 1
	done
'

test_expect_success 'verify state is clean after failed octopus merge' '
	cd merge-state &&
	$REAL_GIT checkout branch1 &&
	test_must_fail $REAL_GIT merge branch2 branch3 2>&1 &&
	$REAL_GIT diff --exit-code --name-status &&
	test_path_is_missing .git/MERGE_HEAD
'

test_expect_success 'grit can read the repo state' '
	cd merge-state &&
	git log --oneline >output &&
	test -s output
'

test_done
