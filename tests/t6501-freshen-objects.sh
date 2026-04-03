#!/bin/sh

test_description='check that objects are properly maintained'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m base
'

test_expect_success 'objects are reachable after commit' '
	git fsck &&
	git rev-parse HEAD >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'abandoned branch objects exist until gc' '
	git checkout -b experiment &&
	echo experiment >file &&
	git add file &&
	git commit -m experiment &&
	exp_tree=$(git rev-parse HEAD^{tree}) &&
	git checkout main &&
	git branch -D experiment &&
	git cat-file -t $exp_tree
'

test_expect_success 'write-tree creates valid tree' '
	echo new >newfile &&
	git add newfile &&
	tree=$(git write-tree) &&
	git cat-file -t $tree >actual &&
	echo tree >expect &&
	test_cmp expect actual
'

test_expect_success 'hash-object creates blob' '
	echo "test content" >testblob &&
	blob=$(git hash-object -w testblob) &&
	git cat-file -t $blob >actual &&
	echo blob >expect &&
	test_cmp expect actual
'

test_expect_success 'cat-file -p shows blob content' '
	echo "test content" >testblob &&
	blob=$(git hash-object -w testblob) &&
	git cat-file -p $blob >actual &&
	test_cmp testblob actual
'

test_expect_success 'read-tree and write-tree round-trip' '
	tree1=$(git write-tree) &&
	git read-tree $tree1 &&
	tree2=$(git write-tree) &&
	test "$tree1" = "$tree2"
'

test_done
