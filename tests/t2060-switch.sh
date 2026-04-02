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

test_expect_success 'suggestion to detach' '
	cd repo &&
	git switch master &&
	test_must_fail git switch "master^{commit}" 2>stderr &&
	grep "try again with the --detach option" stderr
'

test_expect_success 'suggestion to detach is suppressed with advice.suggestDetachingHead=false' '
	cd repo &&
	git switch master &&
	git config advice.suggestDetachingHead false &&
	test_must_fail git switch "master^{commit}" 2>stderr &&
	test_must_fail grep "try again with the --detach option" stderr &&
	git config --unset advice.suggestDetachingHead
'

test_expect_success 'force create branch from HEAD' '
	cd repo &&
	git switch master &&
	# Create the branch at a different commit so -c will fail
	parent=$(git rev-parse HEAD~1) &&
	git branch force-test "$parent" &&
	git switch --detach master &&
	# -c should fail because force-test already exists
	test_must_fail git switch -c force-test &&
	# --force-create should succeed and overwrite
	git switch --force-create force-test &&
	test "$(git rev-parse master)" = "$(git rev-parse force-test)" &&
	echo refs/heads/force-test >expected-branch &&
	git symbolic-ref HEAD >actual-branch &&
	test_cmp expected-branch actual-branch &&
	git switch master
'

test_expect_success 'switch -c fails when branch already exists' '
	cd repo &&
	git switch master &&
	test_must_fail git switch -c first-branch
'

test_expect_success 'switch --force-create overwrites existing branch' '
	cd repo &&
	git switch master &&
	git switch --force-create first-branch &&
	test "$(git rev-parse master)" = "$(git rev-parse first-branch)" &&
	git switch master
'

test_expect_success 'switch --no-guess does not find remote tracking branch' '
	cd repo &&
	git switch master &&
	test_must_fail git switch --no-guess nonexistent-branch
'

test_done
