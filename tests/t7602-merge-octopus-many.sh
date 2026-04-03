#!/bin/sh
# Ported from upstream git t7602-merge-octopus-many.sh
# Use real git for merge, verify with grit

test_description='octopus merge (via /usr/bin/git), verified with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup' '
	$REAL_GIT init octopus-repo &&
	cd octopus-repo &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	echo base >file &&
	$REAL_GIT add file &&
	test_tick &&
	$REAL_GIT commit -m base
'

test_expect_success 'create branches for octopus merge' '
	cd octopus-repo &&
	for i in 1 2 3 4 5; do
		$REAL_GIT checkout -b branch$i master &&
		echo "content from branch $i" >file$i &&
		$REAL_GIT add file$i &&
		test_tick &&
		$REAL_GIT commit -m "branch $i" || return 1
	done &&
	$REAL_GIT checkout master
'

test_expect_success 'octopus merge' '
	cd octopus-repo &&
	$REAL_GIT merge branch1 branch2 branch3 branch4 branch5 &&
	# All files should be present
	test -f file1 &&
	test -f file2 &&
	test -f file3 &&
	test -f file4 &&
	test -f file5
'

test_expect_success 'grit log shows merge' '
	cd octopus-repo &&
	git log --oneline >actual &&
	grep "Merge" actual
'

test_expect_success 'grit rev-parse HEAD works after octopus' '
	cd octopus-repo &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'grit cat-file on merge commit' '
	cd octopus-repo &&
	git cat-file -p HEAD >actual &&
	# Octopus merge should have multiple parents
	parent_count=$(grep "^parent " actual | wc -l) &&
	test "$parent_count" -ge 2
'

test_expect_success 'grit branch lists branches' '
	cd octopus-repo &&
	git branch >actual &&
	grep "master" actual &&
	grep "branch1" actual
'

test_expect_success 'grit diff HEAD shows clean after merge' '
	cd octopus-repo &&
	git diff HEAD >actual &&
	test_must_be_empty actual
'

test_done
