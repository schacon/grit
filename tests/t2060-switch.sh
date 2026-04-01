#!/bin/sh
# Ported subset from git/t/t2060-switch.sh

test_description='switch basic functionality'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Note: grit uses 'master' as default branch, not 'main'

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo first >first.t &&
	git add first.t &&
	git commit -m first &&
	git tag first &&
	git branch first-branch &&
	echo second >second.t &&
	git add second.t &&
	git commit -m second &&
	git tag second &&
	echo third >third.t &&
	git add third.t &&
	git commit -m third &&
	git tag third
'

test_expect_success 'switch branch no arguments' '
	cd repo &&
	test_must_fail git switch
'

test_expect_success 'switch branch' '
	cd repo &&
	git switch first-branch &&
	test_path_is_missing second.t
'

test_expect_success 'switch and detach' '
	cd repo &&
	git switch master &&
	test_must_fail git switch "master^{commit}" &&
	git switch --detach "master^{commit}" &&
	test_must_fail git symbolic-ref HEAD
'

test_expect_success 'switch and detach current branch' '
	cd repo &&
	git switch master &&
	git switch --detach &&
	test_must_fail git symbolic-ref HEAD
'

test_expect_success 'switch and create branch' '
	cd repo &&
	git switch master &&
	git switch -c temp "master^" &&
	echo refs/heads/temp >expected-branch &&
	git symbolic-ref HEAD >actual-branch &&
	test_cmp expected-branch actual-branch
'

test_expect_success 'new orphan branch from empty' '
	cd repo &&
	git switch master &&
	test_must_fail git switch --orphan new-orphan HEAD &&
	git switch --orphan new-orphan &&
	git ls-files >tracked-files &&
	test_must_be_empty tracked-files
'

test_expect_success 'orphan branch works with --discard-changes' '
	cd repo &&
	git switch master &&
	echo foo >foo.txt &&
	git switch --discard-changes --orphan new-orphan2 &&
	git ls-files >tracked-files &&
	test_must_be_empty tracked-files
'

test_expect_success 'switching ignores file of same branch name' '
	cd repo &&
	git switch master &&
	: >first-branch &&
	git switch first-branch &&
	echo refs/heads/first-branch >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'not switching when something is in progress' '
	cd repo &&
	git switch master &&
	cp .git/HEAD .git/MERGE_HEAD &&
	test_must_fail git switch -d "@^" &&
	rm -f .git/MERGE_HEAD
'

test_done
