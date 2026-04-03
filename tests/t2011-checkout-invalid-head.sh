#!/bin/sh
# Test checkout behavior with invalid or missing HEAD.

test_description='grit checkout with invalid/missing HEAD'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Setup
###########################################################################

test_expect_success 'setup: create repo with history' '
	grit init ck-repo &&
	cd ck-repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&
	echo "first" >file.txt &&
	grit add file.txt &&
	grit commit -m "first" &&
	echo "second" >file.txt &&
	grit add file.txt &&
	grit commit -m "second" &&
	grit checkout -b side &&
	echo "side" >side.txt &&
	grit add side.txt &&
	grit commit -m "side" &&
	grit checkout master
'

###########################################################################
# Section 2: Corrupt/missing HEAD
###########################################################################

test_expect_failure 'checkout fails gracefully with empty HEAD file' '
	cd ck-repo &&
	cp .git/HEAD .git/HEAD.bak &&
	: >.git/HEAD &&
	test_must_fail grit checkout master 2>err &&
	cp .git/HEAD.bak .git/HEAD
'

test_expect_failure 'checkout fails gracefully with garbage HEAD' '
	cd ck-repo &&
	cp .git/HEAD .git/HEAD.bak &&
	echo "garbage" >.git/HEAD &&
	test_must_fail grit checkout master 2>err &&
	cp .git/HEAD.bak .git/HEAD
'

test_expect_success 'HEAD restored to valid state after corruption tests' '
	cd ck-repo &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/master" out
'

###########################################################################
# Section 3: Checkout to nonexistent branch/ref
###########################################################################

test_expect_success 'checkout nonexistent branch fails' '
	cd ck-repo &&
	test_must_fail grit checkout nonexistent-branch 2>err
'

test_expect_success 'checkout -b creates branch even with detached HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	grit checkout -b detached-new &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/detached-new" out &&
	grit checkout master
'

test_expect_success 'checkout to invalid ref syntax fails' '
	cd ck-repo &&
	test_must_fail grit checkout "refs/heads/../bad" 2>err
'

###########################################################################
# Section 4: Detached HEAD operations
###########################################################################

test_expect_success 'checkout to commit SHA detaches HEAD' '
	cd ck-repo &&
	HEAD_OID=$(grit rev-parse HEAD) &&
	grit checkout "$HEAD_OID" &&
	test_must_fail grit symbolic-ref HEAD 2>err &&
	grit checkout master
'

test_expect_success 'checkout HEAD~N detaches HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	test_must_fail grit symbolic-ref HEAD 2>err &&
	grit checkout master
'

test_expect_success 'symbolic-ref reports error on detached HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	test_must_fail grit symbolic-ref HEAD 2>err &&
	grep -i "not a symbolic ref" err &&
	grit checkout master
'

test_expect_success 'show-ref still works with detached HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	grit show-ref --heads >out &&
	grep "refs/heads/master" out &&
	grit checkout master
'

test_expect_success 'checkout back to branch from detached HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	test_must_fail grit symbolic-ref HEAD &&
	grit checkout master &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/master" out
'

###########################################################################
# Section 5: Orphan branch
###########################################################################

test_expect_success 'checkout --orphan creates orphan branch' '
	cd ck-repo &&
	grit checkout --orphan orphan-branch &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/orphan-branch" out &&
	test_must_fail grit show-ref --verify refs/heads/orphan-branch
'

test_expect_success 'checkout --orphan with existing branch name fails or warns' '
	cd ck-repo &&
	grit checkout master &&
	test_must_fail grit checkout --orphan master 2>err
'

test_expect_success 'return to master after orphan' '
	cd ck-repo &&
	grit checkout master &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/master" out
'

###########################################################################
# Section 6: Checkout -- file with invalid HEAD
###########################################################################

test_expect_success 'checkout -- file works from index even with detached HEAD' '
	cd ck-repo &&
	grit checkout HEAD~1 &&
	echo "modified" >file.txt &&
	grit checkout -- file.txt &&
	echo "first" >expected &&
	test_cmp expected file.txt &&
	grit checkout master
'

test_expect_success 'checkout -- restores deleted file' '
	cd ck-repo &&
	rm file.txt &&
	grit checkout -- file.txt &&
	test -f file.txt
'

test_expect_success 'checkout -- nonexistent file fails' '
	cd ck-repo &&
	test_must_fail grit checkout -- no-such-file.txt 2>err
'

###########################################################################
# Section 7: Checkout -f (force)
###########################################################################

test_expect_success 'checkout -f discards uncommitted changes' '
	cd ck-repo &&
	echo "dirty" >file.txt &&
	grit checkout -f master &&
	echo "second" >expected &&
	test_cmp expected file.txt
'

test_expect_success 'checkout -f to another branch discards changes' '
	cd ck-repo &&
	echo "dirty" >file.txt &&
	grit checkout -f side &&
	grit symbolic-ref HEAD >out &&
	grep "refs/heads/side" out &&
	grit checkout master
'

###########################################################################
# Section 8: Edge cases
###########################################################################

test_expect_success 'checkout branch same as current is no-op' '
	cd ck-repo &&
	grit checkout master 2>out &&
	grit symbolic-ref HEAD >ref &&
	grep "refs/heads/master" ref
'

test_expect_failure 'checkout with empty string argument fails' '
	cd ck-repo &&
	test_must_fail grit checkout "" 2>err
'

test_expect_success 'HEAD points to correct commit after multiple checkouts' '
	cd ck-repo &&
	grit checkout side &&
	grit checkout master &&
	grit checkout side &&
	grit checkout master &&
	MASTER_OID=$(grit rev-parse master) &&
	HEAD_OID=$(grit rev-parse HEAD) &&
	test "$MASTER_OID" = "$HEAD_OID"
'

test_expect_success 'checkout -b from detached HEAD creates correct branch' '
	cd ck-repo &&
	FIRST=$(grit rev-list --reverse HEAD | head -1) &&
	grit checkout "$FIRST" &&
	grit checkout -b from-first &&
	BRANCH_OID=$(grit rev-parse from-first) &&
	test "$FIRST" = "$BRANCH_OID" &&
	grit checkout master
'

test_done
