#!/bin/sh

test_description='test worktree writing operations when skip-worktree is used'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo init >init.t &&
	git add init.t &&
	git commit -m init &&
	git tag init &&
	echo modified >>init.t &&
	echo added >added &&
	git add init.t added &&
	git commit -m "modified and added" &&
	git tag top
'

test_expect_success 'read-tree updates worktree, absent case' '
	git checkout -f top &&
	git update-index --skip-worktree init.t &&
	rm init.t &&
	parent_tree=$(git rev-parse HEAD^^{tree}) &&
	git read-tree -m -u $parent_tree &&
	echo init >expected &&
	test_cmp expected init.t
'

# skip-worktree + read-tree removal: grit's read-tree -m -u
# does not currently skip absent skip-worktree entries

test_expect_success 'index setup with skip-worktree' '
	git checkout -f init &&
	mkdir -p sub &&
	echo 1 >1 &&
	echo 2 >2 &&
	echo sub1 >sub/1 &&
	echo sub2 >sub/2 &&
	git add 1 2 sub/1 sub/2 &&
	git commit -m "add files" &&
	git update-index --skip-worktree 1 sub/1 &&
	git ls-files --stage 1 >actual &&
	grep "1$" actual
'

test_expect_success 'git-rm fails if skip-worktree and dirty' '
	echo dirty >1 &&
	test_must_fail git rm 1
'

test_expect_success 'git clean removes untracked files' '
	echo untracked >untracked-file &&
	git clean -f &&
	test_path_is_missing untracked-file
'

test_done
