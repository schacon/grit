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

# ---------------------------------------------------------------------------
# Additional switch tests
# ---------------------------------------------------------------------------

test_expect_success 'switch -c with start-point' '
	cd repo &&
	git switch master &&
	git switch -c new-at-first first &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse first)" &&
	echo refs/heads/new-at-first >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch -c with start-point keeps files correct' '
	cd repo &&
	git switch master &&
	git switch -c at-first-check first &&
	test_path_is_file first.t &&
	test_path_is_missing second.t &&
	test_path_is_missing third.t &&
	git switch master
'

test_expect_success 'switch to non-existent branch fails' '
	cd repo &&
	git switch master &&
	test_must_fail git switch nosuchbranch
'

test_expect_success 'switch to invalid branch name fails' '
	cd repo &&
	git switch master &&
	test_must_fail git switch -c "bad..name"
'

test_expect_success 'switch to current branch says already on' '
	cd repo &&
	git switch master 2>stderr &&
	grep -i "already on" stderr
'

test_expect_success 'switch - goes to previous branch' '
	cd repo &&
	git switch master &&
	git switch first-branch &&
	git switch master &&
	git switch - &&
	echo refs/heads/first-branch >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch --detach HEAD works' '
	cd repo &&
	git switch master &&
	git switch --detach HEAD &&
	test_must_fail git symbolic-ref HEAD &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	git switch master
'

test_expect_success 'switch --detach with tag' '
	cd repo &&
	git switch master &&
	git switch --detach second &&
	test_must_fail git symbolic-ref HEAD &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	git switch master
'

test_expect_success 'switch --detach with branch~N' '
	cd repo &&
	git switch master &&
	git switch --detach "master~1" &&
	test_must_fail git symbolic-ref HEAD &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	git switch master
'

test_expect_success 'switch to tag without --detach fails with hint' '
	cd repo &&
	git switch master &&
	test_must_fail git switch second 2>stderr &&
	grep "detach" stderr
'

test_expect_success 'switch --orphan creates branch with no files' '
	cd repo &&
	git switch master &&
	git switch --orphan clean-orphan &&
	git ls-files >tracked &&
	test_must_be_empty tracked &&
	git switch master
'

test_expect_success 'switch --orphan with start-point fails' '
	cd repo &&
	git switch master &&
	test_must_fail git switch --orphan bad-orphan HEAD
'

test_expect_success 'switch --force-create moves existing branch to HEAD' '
	cd repo &&
	git switch master &&
	git branch target-branch first &&
	test "$(git rev-parse target-branch)" = "$(git rev-parse first)" &&
	git switch --force-create target-branch &&
	test "$(git rev-parse target-branch)" = "$(git rev-parse master)" &&
	git switch master
'

test_expect_success 'switch --force-create creates new branch if not existing' '
	cd repo &&
	git switch master &&
	git switch --force-create brand-new-fc &&
	echo refs/heads/brand-new-fc >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_done
