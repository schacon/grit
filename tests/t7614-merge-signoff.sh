#!/bin/sh

test_description='git merge --signoff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init merge-signoff &&
	cd merge-signoff &&
	$REAL_GIT config user.name "C O Mitter" &&
	$REAL_GIT config user.email "committer@example.com" &&

	$REAL_GIT commit --allow-empty -m "Initial empty commit" &&
	$REAL_GIT checkout -b other-branch &&
	echo file1content >file1 &&
	$REAL_GIT add file1 &&
	test_tick &&
	$REAL_GIT commit -m "other-branch"
'

test_expect_success 'merge --signoff adds sign-off line' '
	cd merge-signoff &&
	$REAL_GIT checkout main 2>/dev/null || $REAL_GIT checkout master &&
	echo file2content >file2 &&
	$REAL_GIT add file2 &&
	test_tick &&
	$REAL_GIT commit -m "main-branch-2" &&
	$REAL_GIT merge other-branch --signoff --no-edit &&
	git cat-file commit HEAD >raw &&
	sed -e "1,/^$/d" raw >actual &&
	grep "Signed-off-by:" actual
'

test_expect_success 'merge without --signoff has no sign-off' '
	cd merge-signoff &&
	$REAL_GIT checkout -b no-signoff other-branch &&
	echo file3content >file3 &&
	$REAL_GIT add file3 &&
	test_tick &&
	$REAL_GIT commit -m "no-signoff" &&
	$REAL_GIT checkout main 2>/dev/null || $REAL_GIT checkout master &&
	echo file4content >file4 &&
	$REAL_GIT add file4 &&
	test_tick &&
	$REAL_GIT commit -m "main-branch-3" &&
	$REAL_GIT merge no-signoff --no-edit &&
	git cat-file commit HEAD >raw &&
	sed -e "1,/^$/d" raw >actual &&
	! grep "Signed-off-by:" actual
'

test_done
