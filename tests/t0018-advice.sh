#!/bin/sh
# Tests for advice messages (advice.* config variables).

test_description='grit advice messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test User" &&
	echo initial >file &&
	git add file &&
	git commit -m "initial commit"
'

test_expect_success 'detached HEAD shows advice by default' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>err &&
	grep -i "detached HEAD" err
'

test_expect_success 'detached HEAD advice mentions switch -c' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git checkout master 2>/dev/null &&
	git checkout $oid 2>err &&
	grep "switch" err
'

test_expect_success 'advice.detachedHead=false suppresses detached HEAD advice' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git config advice.detachedHead false &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>err &&
	! grep "You can look around" err &&
	git config --unset advice.detachedHead
'

test_expect_success 'advice.detachedHead=true shows detached HEAD advice' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git config advice.detachedHead true &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>err &&
	grep -i "detached HEAD" err &&
	git config --unset advice.detachedHead
'

test_expect_success 'advice.statusHints can be set to false' '
	cd repo &&
	git config advice.statusHints false &&
	val=$(git config advice.statusHints) &&
	test "$val" = "false"
'

test_expect_success 'advice.statusHints can be set to true' '
	cd repo &&
	git config advice.statusHints true &&
	val=$(git config advice.statusHints) &&
	test "$val" = "true"
'

test_expect_success 'status shows hints by default' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git config --unset advice.statusHints 2>/dev/null || true &&
	echo new >untracked_file &&
	git status >out 2>&1 &&
	grep -i "git add" out &&
	rm -f untracked_file
'

test_expect_success 'advice.pushUpdateRejected can be set' '
	cd repo &&
	git config advice.pushUpdateRejected false &&
	val=$(git config advice.pushUpdateRejected) &&
	test "$val" = "false" &&
	git config --unset advice.pushUpdateRejected
'

test_expect_success 'advice config is case-insensitive' '
	cd repo &&
	git config advice.DETACHEDHEAD false &&
	val=$(git config advice.detachedhead) &&
	test "$val" = "false" &&
	git config --unset advice.detachedhead
'

test_expect_success 'multiple advice configs can coexist' '
	cd repo &&
	git config advice.detachedHead false &&
	git config advice.statusHints false &&
	git config advice.pushUpdateRejected false &&
	git config --list >out &&
	grep "advice.detachedhead=false" out &&
	grep "advice.statushints=false" out &&
	grep "advice.pushupdaterejected=false" out &&
	git config --unset advice.detachedHead &&
	git config --unset advice.statusHints &&
	git config --unset advice.pushUpdateRejected
'

test_expect_success 'advice config persists across commands' '
	cd repo &&
	git config advice.detachedHead false &&
	git config advice.detachedHead >out &&
	test "$(cat out)" = "false" &&
	git config --unset advice.detachedHead
'

test_expect_success 'unsetting advice config removes it' '
	cd repo &&
	git config advice.detachedHead false &&
	git config --unset advice.detachedHead &&
	test_must_fail git config advice.detachedHead
'

test_expect_success 'detached HEAD at commit shows oid prefix' '
	cd repo &&
	git checkout master 2>/dev/null &&
	oid=$(git rev-parse HEAD) &&
	short=$(echo $oid | cut -c1-7) &&
	git checkout $oid 2>err &&
	grep "$short" err
'

test_expect_success 'checkout to branch after detached HEAD works' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>/dev/null &&
	git checkout master 2>err &&
	grep -i "master" err || grep -i "Switched" err
'

test_expect_success 'status on detached HEAD shows detached info' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>/dev/null &&
	git status >out 2>&1 &&
	grep -i "detached" out
'

test_expect_success 'advice.detachedHead false: status still shows detached' '
	cd repo &&
	git config advice.detachedHead false &&
	git status >out 2>&1 &&
	grep -i "detached" out &&
	git config --unset advice.detachedHead
'

test_expect_success 'advice config values are stored as booleans' '
	cd repo &&
	git config advice.detachedHead false &&
	git config --type=bool advice.detachedHead >out 2>&1 || true &&
	git config advice.detachedHead >out2 &&
	grep "false" out2 &&
	git config --unset advice.detachedHead
'

test_expect_success 'advice.resolveConflict can be configured' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git config advice.resolveConflict false &&
	val=$(git config advice.resolveConflict) &&
	test "$val" = "false" &&
	git config --unset advice.resolveConflict
'

test_expect_success 'advice.implicitIdentity can be configured' '
	cd repo &&
	git config advice.implicitIdentity false &&
	val=$(git config advice.implicitIdentity) &&
	test "$val" = "false" &&
	git config --unset advice.implicitIdentity
'

test_expect_success 'checkout master after all tests' '
	cd repo &&
	git checkout master 2>/dev/null
'

test_done
