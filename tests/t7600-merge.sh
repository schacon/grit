#!/bin/sh
# Ported from upstream git t7600-merge.sh
# grit may not have full merge, use /usr/bin/git for merging,
# verify results with grit.

test_description='merge (via /usr/bin/git), verified with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup merge repo' '
	$REAL_GIT init merge-repo &&
	cd merge-repo &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@example.com" &&
	echo "base content" >file.txt &&
	$REAL_GIT add file.txt &&
	test_tick &&
	$REAL_GIT commit -m "base commit"
'

test_expect_success 'create divergent branches' '
	cd merge-repo &&
	$REAL_GIT checkout -b feature &&
	echo "feature line" >>file.txt &&
	$REAL_GIT add file.txt &&
	test_tick &&
	$REAL_GIT commit -m "feature commit" &&
	$REAL_GIT checkout master &&
	echo "master line" >file2.txt &&
	$REAL_GIT add file2.txt &&
	test_tick &&
	$REAL_GIT commit -m "master commit"
'

test_expect_success 'merge feature into master' '
	cd merge-repo &&
	$REAL_GIT merge feature -m "merge feature"
'

test_expect_success 'grit log shows merge commit' '
	cd merge-repo &&
	git log --oneline >actual &&
	test $(wc -l <actual) -ge 3
'

test_expect_success 'grit cat-file shows merge parents' '
	cd merge-repo &&
	git cat-file -p HEAD >actual &&
	parent_count=$(grep "^parent " actual | wc -l) &&
	test "$parent_count" -eq 2
'

test_expect_success 'grit rev-parse HEAD works' '
	cd merge-repo &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'grit diff HEAD is clean after merge' '
	cd merge-repo &&
	git diff HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'grit status after clean merge' '
	cd merge-repo &&
	git status >actual &&
	grep "On branch" actual
'

test_expect_success 'grit branch shows branches' '
	cd merge-repo &&
	git branch >actual &&
	grep "master" actual &&
	grep "feature" actual
'

test_expect_success 'grit log --format shows subjects' '
	cd merge-repo &&
	git log --format="%s" >actual &&
	grep "merge\|base\|feature\|master" actual
'

test_expect_success 'ff merge' '
	cd merge-repo &&
	$REAL_GIT checkout -b ff-branch &&
	echo "ff content" >ff-file.txt &&
	$REAL_GIT add ff-file.txt &&
	test_tick &&
	$REAL_GIT commit -m "ff commit" &&
	$REAL_GIT checkout master &&
	$REAL_GIT merge ff-branch &&
	git log --oneline >actual &&
	grep "ff commit" actual
'

test_expect_success 'grit merge-base finds common ancestor' '
	cd merge-repo &&
	git merge-base master feature >actual &&
	test -s actual
'

test_done
