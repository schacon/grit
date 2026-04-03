#!/bin/sh
# Ported from upstream git t7610-mergetool.sh
# Test merge conflict resolution verification with grit

test_description='mergetool (merge conflict verification with grit)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init mergetool-repo &&
	cd mergetool-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo base >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m base
'

test_expect_success 'create merge conflict' '
	cd mergetool-repo &&
	$REAL_GIT checkout -b branch1 &&
	echo branch1 >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m branch1 &&
	$REAL_GIT checkout master &&
	echo master >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m master &&
	test_must_fail $REAL_GIT merge branch1
'

test_expect_success 'grit status shows conflict' '
	cd mergetool-repo &&
	git status >actual &&
	grep -i "unmerged\|conflict\|both modified" actual
'

test_expect_success 'grit ls-files shows unmerged' '
	cd mergetool-repo &&
	git ls-files -u >actual &&
	test -s actual
'

test_expect_success 'resolve conflict and verify' '
	cd mergetool-repo &&
	echo resolved >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m resolved &&
	git log --oneline >actual &&
	test_line_count = 4 actual
'

test_expect_success 'grit diff after resolution is clean' '
	cd mergetool-repo &&
	git diff >actual &&
	test_must_be_empty actual
'

test_done
