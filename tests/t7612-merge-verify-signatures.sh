#!/bin/sh

test_description='git merge signature verification (basic)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Signature verification requires GPG which grit may not support.
# Test basic merge functionality that would be used alongside signature checks.

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init merge-sig &&
	cd merge-sig &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&

	echo base >file &&
	$REAL_GIT add file &&
	$REAL_GIT commit -m base &&
	$REAL_GIT tag base &&

	$REAL_GIT checkout -b side &&
	echo side >side-file &&
	$REAL_GIT add side-file &&
	$REAL_GIT commit -m side &&

	$REAL_GIT checkout master &&
	echo main >main-file &&
	$REAL_GIT add main-file &&
	$REAL_GIT commit -m main
'

test_expect_success 'grit reads commit from merged repo' '
	cd merge-sig &&
	$REAL_GIT merge --no-edit side &&
	git cat-file commit HEAD >output &&
	grep "^parent " output >parents &&
	test $(wc -l <parents) -eq 2
'

test_expect_success 'grit reads merge commit message' '
	cd merge-sig &&
	git cat-file commit HEAD >output &&
	grep "Merge" output
'

test_done
