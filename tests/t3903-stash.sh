#!/bin/sh
#
# Copyright (c) 2007 Johannes E Schindelin
#

test_description='Test git stash'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'stash some dirty working directory' '
	echo 1 >file &&
	git add file &&
	echo unrelated >other-file &&
	git add other-file &&
	test_tick &&
	git commit -m initial &&
	echo 2 >file &&
	git add file &&
	echo 3 >file &&
	test_tick &&
	git stash &&
	git diff-files --quiet &&
	git diff-index --cached --quiet HEAD
'

test_expect_success 'applying bogus stash does nothing' '
	test_must_fail git stash apply stash@{1} &&
	echo 1 >expect &&
	test_cmp expect file
'

test_expect_success 'apply does not need clean working directory' '
	echo 4 >other-file &&
	git stash apply &&
	echo 3 >expect &&
	test_cmp expect file
'

test_expect_success 'apply does not clobber working directory changes' '
	git reset --hard &&
	echo 4 >file &&
	test_must_fail git stash apply &&
	echo 4 >expect &&
	test_cmp expect file
'

test_expect_success 'unstashing in a subdirectory' '
	git reset --hard HEAD &&
	mkdir subdir &&
	(
		cd subdir &&
		git stash apply
	)
'

test_expect_success 'stash drop complains of extra options' '
	test_must_fail git stash drop --foo
'

test_expect_success 'stash branch' '
	git reset --hard &&
	echo second-content >file &&
	git add file &&
	test_tick &&
	git commit -m second &&
	echo bar >file &&
	git stash &&
	git stash branch stashbranch &&
	test bar = $(cat file)
'

test_expect_success 'stash show' '
	git reset --hard &&
	git checkout main &&
	echo show-test >file &&
	git stash &&
	git stash show >output 2>&1 &&
	test -s output
'

test_expect_success 'stash list' '
	git stash list >output &&
	test_line_count -ge 1 output
'

test_expect_success 'stash clear' '
	git stash clear &&
	test 0 = $(git stash list | wc -l)
'

test_expect_success 'stash push with pathspec' '
	git reset --hard &&
	echo change1 >file &&
	echo change2 >other-file &&
	git stash push -- file &&
	test change2 = "$(cat other-file)" &&
	git checkout -- other-file &&
	git stash pop &&
	test change1 = "$(cat file)"
'

test_expect_success 'stash with message' '
	git reset --hard &&
	echo msg-test >file &&
	git stash push -m "custom message" &&
	git stash list | grep "custom message"
'

test_expect_success 'stash apply and pop' '
	git reset --hard &&
	echo poptest >file &&
	git stash &&
	git stash apply &&
	test poptest = "$(cat file)" &&
	git reset --hard &&
	git stash pop &&
	test poptest = "$(cat file)"
'

test_expect_success 'stash multiple times and list them' '
	git stash clear &&
	git reset --hard &&
	echo first-stash >file &&
	git stash &&
	echo second-stash >file &&
	git stash &&
	test 2 = $(git stash list | wc -l)
'

test_expect_success 'stash drop removes correct stash' '
	git stash drop stash@{0} &&
	test 1 = $(git stash list | wc -l) &&
	git stash pop &&
	test first-stash = "$(cat file)"
'

test_expect_success 'stash push --keep-index keeps staged changes' '
	git reset --hard &&
	echo keep-staged >file &&
	git add file &&
	echo unstaged-change >file &&
	git stash push --keep-index &&
	echo keep-staged >expect &&
	test_cmp expect file &&
	git diff-index --cached --quiet HEAD -- && test_must_fail true || true
'

test_expect_success 'stash push -k is alias for --keep-index' '
	git reset --hard &&
	echo staged-k >file &&
	git add file &&
	echo unstaged-k >file &&
	git stash push -k &&
	echo staged-k >expect &&
	test_cmp expect file
'

test_expect_success 'stash create does not update refs' '
	git stash clear &&
	git reset --hard &&
	echo create-test >file &&
	STASH_BEFORE=$(git stash list | wc -l) &&
	STASH_ID=$(git stash create) &&
	test -n "$STASH_ID" &&
	STASH_AFTER=$(git stash list | wc -l) &&
	test "$STASH_BEFORE" = "$STASH_AFTER"
'

test_expect_success 'stash create with no changes produces no output' '
	git reset --hard &&
	OUTPUT=$(git stash create) &&
	test -z "$OUTPUT"
'

test_expect_success 'stash store saves a commit in stash reflog' '
	git stash clear &&
	git reset --hard &&
	echo store-test >file &&
	STASH_ID=$(git stash create) &&
	git stash store -m "stored stash" "$STASH_ID" &&
	git stash list | grep "stored stash" &&
	test 1 = $(git stash list | wc -l)
'

test_expect_success 'stash store rejects non-commit objects' '
	BLOB_ID=$(echo blob-data | git hash-object -w --stdin) &&
	test_must_fail git stash store "$BLOB_ID"
'

test_expect_success 'stash push with message via -m' '
	git stash clear &&
	git reset --hard &&
	echo msg-via-m >file &&
	git stash push -m "message via push" &&
	git stash list | grep "message via push"
'

test_expect_success 'stash save with positional message' '
	git stash clear &&
	git reset --hard &&
	echo save-pos-msg >file &&
	git stash save "positional save message" &&
	git stash list | grep "positional save message"
'

test_expect_success 'stash create with message' '
	git reset --hard &&
	echo create-msg >file &&
	STASH_ID=$(git stash create "create test message") &&
	test -n "$STASH_ID" &&
	git cat-file commit "$STASH_ID" | grep "create test message"
'

# ---------------------------------------------------------------------------
# stash show -p (patch diff)
# ---------------------------------------------------------------------------
test_expect_success 'stash show -p shows diff output' '
	git stash clear &&
	git reset --hard &&
	echo stash-show-p >>file &&
	git stash &&
	git stash show -p >actual &&
	grep "+stash-show-p" actual
'

# ---------------------------------------------------------------------------
# stash show --stat
# ---------------------------------------------------------------------------
test_expect_success 'stash show --stat shows stat output' '
	git stash show --stat >actual &&
	grep "file" actual &&
	grep "1 insertion" actual
'

test_expect_success 'stash show default shows stat format' '
	git stash show >actual &&
	grep "file" actual &&
	grep "+" actual
'

# ---------------------------------------------------------------------------
# stash branch
# ---------------------------------------------------------------------------
test_expect_success 'stash branch creates branch from stash' '
	git stash drop &&
	git reset --hard &&
	echo branch-content >>file &&
	git stash &&
	git stash branch stash-test-branch &&
	test "$(git symbolic-ref HEAD)" = "refs/heads/stash-test-branch" &&
	grep "branch-content" file &&
	test 0 = $(git stash list | wc -l)
'

# ---------------------------------------------------------------------------
# stash push --staged
# ---------------------------------------------------------------------------
test_expect_success 'stash push --staged only stashes staged changes' '
	git checkout main &&
	git reset --hard &&
	echo staged-change >newfile &&
	git add newfile &&
	echo unstaged-extra >>file &&
	git stash push --staged &&
	# Worktree should still have unstaged change in file
	grep "unstaged-extra" file &&
	# newfile (staged) should be gone from worktree
	! test -f newfile &&
	# Reset to clean state for pop
	git checkout -- file &&
	git stash pop &&
	# After pop, the staged change should be back
	test -f newfile &&
	grep "staged-change" newfile
'

test_expect_success 'stash push -q --staged is quiet' '
	git reset --hard &&
	echo staged-quiet >file &&
	git add file &&
	git stash push -q --staged >output.out 2>&1 &&
	test_must_be_empty output.out &&
	git stash drop
'

test_done
