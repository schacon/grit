#!/bin/sh

test_description='test cherry-pick and revert with signoff'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m one &&
	git tag one &&

	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m two &&
	git tag two &&

	echo three >file &&
	git add file &&
	test_tick &&
	git commit -m three &&
	git tag three &&

	echo tip >file &&
	git add file &&
	test_tick &&
	git commit -m tip &&
	git tag tip
'

test_expect_success 'cherry-pick with --signoff adds Signed-off-by' '
	git checkout one &&
	echo new-content >newfile &&
	git add newfile &&
	test_tick &&
	git commit -m "new file" &&
	git tag new-file &&

	git checkout tip &&
	git cherry-pick --signoff new-file &&
	git log --format=%B --max-count=1 >msg &&
	grep "Signed-off-by:" msg
'

test_expect_success 'cherry-pick without --signoff has no trailer' '
	git checkout tip &&
	git cherry-pick new-file &&
	git log --format=%B --max-count=1 >msg &&
	! grep "Signed-off-by:" msg
'

test_expect_success 'revert creates correct commit message' '
	git checkout tip &&
	git revert tip &&
	git log --format=%s --max-count=1 >msg &&
	grep "Revert" msg
'

test_expect_success 'cherry-pick -x adds cherry-picked-from' '
	git checkout one &&
	git cherry-pick -x two &&
	git log --format=%B --max-count=1 >msg &&
	grep "cherry picked from commit" msg
'

test_done
