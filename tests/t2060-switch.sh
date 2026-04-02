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

# ---------------------------------------------------------------------------
# Additional switch tests — batch 2
# ---------------------------------------------------------------------------

test_expect_success 'switch -c defaults to HEAD as start-point' '
	cd repo &&
	git switch master &&
	git switch -c at-head-default &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	echo refs/heads/at-head-default >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch --detach with abbreviated SHA' '
	cd repo &&
	git switch master &&
	SHA=$(git rev-parse first) &&
	SHORT=$(git rev-parse --short first) &&
	git switch --detach $SHORT &&
	test "$(git rev-parse HEAD)" = "$SHA" &&
	test_must_fail git symbolic-ref HEAD &&
	git switch master
'

test_expect_success 'switch -c with HEAD as explicit start-point' '
	cd repo &&
	git switch master &&
	git switch -c from-head-explicit HEAD &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	echo refs/heads/from-head-explicit >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switching back and forth preserves worktree correctly' '
	cd repo &&
	git switch master &&
	git branch -f switch-check first &&
	git switch switch-check &&
	test_path_is_file first.t &&
	test_path_is_missing second.t &&
	test_path_is_missing third.t &&
	git switch master &&
	test_path_is_file first.t &&
	test_path_is_file second.t &&
	test_path_is_file third.t
'

test_expect_success 'switch --discard-changes discards staged changes' '
	cd repo &&
	git switch master &&
	echo dirty >>first.t &&
	git add first.t &&
	git switch --discard-changes first-branch &&
	git diff --cached --name-only >staged &&
	test_must_be_empty staged &&
	git switch master
'

test_expect_success 'switch --force-create with explicit start-point moves branch' '
	cd repo &&
	git switch master &&
	git branch test-fc-sp first &&
	test "$(git rev-parse test-fc-sp)" = "$(git rev-parse first)" &&
	git switch --force-create test-fc-sp second &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	echo refs/heads/test-fc-sp >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch from detached HEAD to named branch' '
	cd repo &&
	git switch --detach first &&
	test_must_fail git symbolic-ref HEAD &&
	git switch master &&
	echo refs/heads/master >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'switch --orphan then commit creates parentless commit' '
	cd repo &&
	git switch master &&
	git switch --orphan test-orphan &&
	echo content >orphan-file.t &&
	git add orphan-file.t &&
	git commit -m "orphan commit" &&
	git cat-file -p HEAD >commit-info &&
	test_must_fail grep "^parent" commit-info &&
	git switch master
'

test_expect_success 'switch prefers branch name over directory name' '
	cd repo &&
	git switch master &&
	mkdir -p testdir &&
	git branch testdir first &&
	git switch testdir &&
	echo refs/heads/testdir >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master &&
	rm -rf testdir
'

test_expect_success 'switch -c with tag as start-point creates branch at tag' '
	cd repo &&
	git switch master &&
	git switch -c from-second-tag second &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test_path_is_file first.t &&
	test_path_is_file second.t &&
	test_path_is_missing third.t &&
	git switch master
'

test_expect_success 'switch --detach from already detached state' '
	cd repo &&
	git switch --detach first &&
	git switch --detach second &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	test_must_fail git symbolic-ref HEAD &&
	git switch master
'

test_expect_success 'switch - returns to previous branch after detach' '
	cd repo &&
	git switch first-branch &&
	git switch --detach master &&
	git switch - &&
	echo refs/heads/first-branch >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch to empty orphan clears worktree of tracked files' '
	cd repo &&
	git switch master &&
	test_path_is_file first.t &&
	git switch --orphan empty-orphan &&
	git ls-files >tracked &&
	test_must_be_empty tracked &&
	git switch master
'

test_expect_success 'switch --force-create on current branch resets it' '
	cd repo &&
	git switch master &&
	old_head=$(git rev-parse HEAD) &&
	git switch --force-create master first &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse first)" &&
	git switch --force-create master $old_head &&
	test "$(git rev-parse HEAD)" = "$old_head"
'

test_expect_success 'switch -c fails with empty branch name' '
	cd repo &&
	git switch master &&
	test_must_fail git switch -c ""
'

test_expect_success 'switch -c fails with name containing space' '
	cd repo &&
	git switch master &&
	test_must_fail git switch -c "bad name"
'

test_expect_success 'switch --detach to HEAD is valid' '
	cd repo &&
	git switch master &&
	git switch --detach HEAD &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	test_must_fail git symbolic-ref HEAD &&
	git switch master
'

test_expect_success 'switch to branch updates HEAD symref' '
	cd repo &&
	git switch first-branch &&
	test "$(cat .git/HEAD)" = "ref: refs/heads/first-branch" &&
	git switch master &&
	test "$(cat .git/HEAD)" = "ref: refs/heads/master"
'

test_expect_success 'switch --detach writes raw SHA to HEAD' '
	cd repo &&
	git switch master &&
	git switch --detach second &&
	HEAD_CONTENT=$(cat .git/HEAD) &&
	SHA=$(git rev-parse second) &&
	test "$HEAD_CONTENT" = "$SHA" &&
	git switch master
'

test_expect_success 'switch --track sets up tracking for local branch' '
	cd repo &&
	git switch master &&
	git switch --track -c track-local first-branch &&
	test "$(git config branch.track-local.remote)" = "." &&
	test "$(git config branch.track-local.merge)" = "refs/heads/first-branch" &&
	git switch master
'

test_expect_success 'switch --track=direct sets up direct tracking' '
	cd repo &&
	git switch master &&
	git switch --track=direct -c track-direct first-branch &&
	test "$(git config branch.track-direct.remote)" = "." &&
	test "$(git config branch.track-direct.merge)" = "refs/heads/first-branch" &&
	git switch master
'

test_expect_success 'switch --track=inherit warns when no remote on upstream' '
	cd repo &&
	git switch master &&
	git switch --track=inherit -c track-inherit first-branch 2>stderr &&
	grep -i "inherit" stderr &&
	git switch master
'

test_expect_success 'switch -c without --track does not set up tracking' '
	cd repo &&
	git switch master &&
	git switch -c no-track first-branch &&
	test_must_fail git config branch.no-track.remote &&
	test_must_fail git config branch.no-track.merge &&
	git switch master
'

test_expect_success 'switch -c with start-point at HEAD~1' '
	cd repo &&
	git switch master &&
	git switch -c at-parent "master~1" &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master~1)" &&
	git switch master
'

test_expect_success 'switch from orphan branch back to master' '
	cd repo &&
	git switch --orphan temp-orphan &&
	git ls-files >tracked &&
	test_must_be_empty tracked &&
	git switch master &&
	test_path_is_file first.t &&
	test_path_is_file second.t &&
	test_path_is_file third.t
'

test_expect_success 'switch --discard-changes from orphan to named branch' '
	cd repo &&
	git switch --orphan dirty-orphan &&
	echo new >orphan-dirty.txt &&
	git add orphan-dirty.txt &&
	git switch --discard-changes master &&
	echo refs/heads/master >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'switch -c with HEAD^{commit} notation' '
	cd repo &&
	git switch master &&
	git switch -c from-peel "master^{commit}" &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	git switch master
'

test_expect_success 'switch --detach preserves untracked files' '
	cd repo &&
	git switch master &&
	echo untracked >untracked.txt &&
	git switch --detach first &&
	test -f untracked.txt &&
	test "$(cat untracked.txt)" = "untracked" &&
	rm untracked.txt &&
	git switch master
'

test_expect_success 'switch -c then switch back preserves new branch' '
	cd repo &&
	git switch master &&
	git switch -c persist-test first &&
	git switch master &&
	git rev-parse persist-test &&
	test "$(git rev-parse persist-test)" = "$(git rev-parse first)"
'

test_expect_success 'switch to branch with slashes in name' '
	cd repo &&
	git switch master &&
	git switch -c feature/test-slash first &&
	echo refs/heads/feature/test-slash >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch - after detach goes to last branch' '
	cd repo &&
	git switch master &&
	git switch first-branch &&
	git switch --detach second &&
	git switch - &&
	echo refs/heads/first-branch >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch --force-create with same commit is no-op' '
	cd repo &&
	git switch master &&
	HEAD_BEFORE=$(git rev-parse HEAD) &&
	git switch --force-create master &&
	test "$(git rev-parse HEAD)" = "$HEAD_BEFORE" &&
	echo refs/heads/master >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'switch --discard-changes discards worktree modifications' '
	cd repo &&
	git switch master &&
	echo dirty >first.t &&
	git switch --discard-changes master &&
	test "$(cat first.t)" != "dirty"
'

test_expect_success 'switch multiple times and verify - goes to correct branch' '
	cd repo &&
	git switch master &&
	git switch first-branch &&
	git switch master &&
	git switch first-branch &&
	git switch - &&
	echo refs/heads/master >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch -f is alias for --discard-changes' '
	cd repo &&
	git switch master &&
	echo dirty >first.t &&
	git switch -f first-branch &&
	echo refs/heads/first-branch >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	git switch master
'

test_expect_success 'switch -c with commit SHA as start-point' '
	cd repo &&
	git switch master &&
	SHA=$(git rev-parse first) &&
	git switch -c from-sha "$SHA" &&
	test "$(git rev-parse HEAD)" = "$SHA" &&
	git switch master
'

test_expect_success 'switch --detach with master~2' '
	cd repo &&
	git switch master &&
	git switch --detach "master~2" &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse first)" &&
	test_must_fail git symbolic-ref HEAD &&
	git switch master
'

test_expect_success 'switch --detach from branch shows previous HEAD position' '
	cd repo &&
	git switch master &&
	git switch --detach second 2>stderr &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse second)" &&
	git switch master
'

test_expect_success 'switch --orphan fails if branch already exists' '
	cd repo &&
	git switch master &&
	test_must_fail git switch --orphan first-branch
'

test_expect_success 'switch to tag with --detach updates worktree files' '
	cd repo &&
	git switch master &&
	git switch --detach first &&
	test_path_is_file first.t &&
	test_path_is_missing second.t &&
	test_path_is_missing third.t &&
	git switch master
'

test_expect_success 'switch --track with -c sets upstream for new branch' '
	cd repo &&
	git switch master &&
	git switch --track -c tracked-branch first-branch &&
	test "$(git config branch.tracked-branch.remote)" = "." &&
	test "$(git config branch.tracked-branch.merge)" = "refs/heads/first-branch" &&
	git switch master
'

test_expect_success 'switch to ambiguous name (tag and branch) prefers branch' '
	cd repo &&
	git switch master &&
	git branch ambig-name first &&
	git tag ambig-name second &&
	git switch ambig-name &&
	echo refs/heads/ambig-name >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse first)" &&
	git switch master
'

test_done
