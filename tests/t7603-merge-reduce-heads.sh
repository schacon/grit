#!/bin/sh

test_description='git merge with multiple heads reduction'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init merge-reduce &&
	cd merge-reduce &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&

	echo base >file &&
	$REAL_GIT add file &&
	$REAL_GIT commit -m base &&
	$REAL_GIT tag base &&

	$REAL_GIT checkout -b branch1 &&
	echo branch1 >b1 &&
	$REAL_GIT add b1 &&
	$REAL_GIT commit -m branch1 &&

	$REAL_GIT checkout master &&
	$REAL_GIT checkout -b branch2 &&
	echo branch2 >b2 &&
	$REAL_GIT add b2 &&
	$REAL_GIT commit -m branch2 &&

	$REAL_GIT checkout master &&
	$REAL_GIT checkout -b branch3 &&
	echo branch3 >b3 &&
	$REAL_GIT add b3 &&
	$REAL_GIT commit -m branch3
'

test_expect_success 'grit can read merge-reduce repo' '
	cd merge-reduce &&
	git log --oneline >output &&
	test -s output
'

test_expect_success 'grit can read branches' '
	cd merge-reduce &&
	git branch >output &&
	grep branch1 output &&
	grep branch2 output &&
	grep branch3 output
'

test_expect_success 'grit can read tags' '
	cd merge-reduce &&
	git tag >output &&
	grep base output
'

test_expect_success 'merge two branches into master' '
	cd merge-reduce &&
	$REAL_GIT checkout master &&
	$REAL_GIT merge branch1 branch2 &&
	git log --oneline >output &&
	test -s output
'

test_done
