#!/bin/sh
# Ported from upstream git t8004-blame-with-conflicts.sh

test_description='git blame on conflicted files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup first case' '
	$REAL_GIT init blame-conflict &&
	cd blame-conflict &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@example.com" &&
	echo "Old line" >file1 &&
	$REAL_GIT add file1 &&
	$REAL_GIT commit --author "Old Line <ol@localhost>" -m file1.a &&

	$REAL_GIT checkout -b foo &&
	$REAL_GIT rm file1 &&
	echo "New line ..." >file2 &&
	echo "... and more" >>file2 &&
	$REAL_GIT add file2 &&
	$REAL_GIT commit --author "U Gly <ug@localhost>" -m ugly &&

	$REAL_GIT checkout master &&
	echo "" >>file1 &&
	echo "bla" >>file1 &&
	$REAL_GIT commit --author "Old Line <ol@localhost>" -a -m file1.b &&

	$REAL_GIT checkout foo &&
	if $REAL_GIT merge master; then
		echo needed conflict here
		exit 1
	else
		echo merge failed - resolving automatically
	fi &&
	printf "New line ...\n... and more\n\nbla\nEven more\n" >file2 &&
	$REAL_GIT rm -f file1 &&
	$REAL_GIT commit --author "M Result <mr@localhost>" -a -m merged &&

	$REAL_GIT checkout master &&
	sed s/bla/foo/ <file1 >X &&
	rm file1 && mv X file1 &&
	$REAL_GIT commit --author "No Bla <nb@localhost>" -a -m replace &&

	$REAL_GIT checkout foo &&
	if $REAL_GIT merge master; then
		echo needed conflict here
		exit 1
	else
		echo merge failed - test is setup
	fi
'

test_expect_success 'blame runs on unconflicted file while other file has conflicts' '
	cd blame-conflict &&
	git blame file2
'

test_expect_success 'blame does not crash with conflicted file' '
	cd blame-conflict &&
	git blame file1 || true
'

test_done
