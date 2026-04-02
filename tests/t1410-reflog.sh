#!/bin/sh
#
# Tests for ref updates and reflog-adjacent behaviors — update-ref,
# symbolic-ref, rev-parse HEAD tracking, and ref manipulation.
# Ported subset from git/t/t1410-reflog.sh (upstream ~41 tests).
# grit lacks 'reflog' subcommand so we test ref update mechanics,
# update-ref -m messages, and ref state after various operations.

test_description='grit ref updates and reflog-related behaviors'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with history' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "first" >file &&
	git add file &&
	git commit -m "first commit" &&
	git rev-parse HEAD >../first &&

	echo "second" >file &&
	git add file &&
	git commit -m "second commit" &&
	git rev-parse HEAD >../second &&

	echo "third" >file &&
	git add file &&
	git commit -m "third commit" &&
	git rev-parse HEAD >../third
'

# ---------------------------------------------------------------------------
# update-ref basics
# ---------------------------------------------------------------------------
test_expect_success 'update-ref creates a new ref' '
	cd repo &&
	git update-ref refs/test/new-ref $(cat ../first) &&
	result=$(git rev-parse refs/test/new-ref) &&
	test "$result" = "$(cat ../first)"
'

test_expect_success 'update-ref updates existing ref' '
	cd repo &&
	git update-ref refs/test/new-ref $(cat ../second) &&
	result=$(git rev-parse refs/test/new-ref) &&
	test "$result" = "$(cat ../second)"
'

test_expect_success 'update-ref with old value check succeeds' '
	cd repo &&
	git update-ref refs/test/new-ref $(cat ../third) $(cat ../second) &&
	result=$(git rev-parse refs/test/new-ref) &&
	test "$result" = "$(cat ../third)"
'

test_expect_success 'update-ref with wrong old value fails' '
	cd repo &&
	test_must_fail git update-ref refs/test/new-ref $(cat ../first) $(cat ../second)
'

test_expect_success 'ref unchanged after failed update' '
	cd repo &&
	result=$(git rev-parse refs/test/new-ref) &&
	test "$result" = "$(cat ../third)"
'

# ---------------------------------------------------------------------------
# update-ref delete
# ---------------------------------------------------------------------------
test_expect_success 'update-ref -d deletes a ref' '
	cd repo &&
	git update-ref refs/test/to-delete $(cat ../first) &&
	git update-ref -d refs/test/to-delete &&
	test_must_fail git rev-parse --verify refs/test/to-delete
'

test_expect_success 'deleting already-deleted ref is handled' '
	cd repo &&
	# grit may accept this silently; verify the ref does not exist after
	git update-ref -d refs/test/no-such-ref 2>/dev/null;
	test_must_fail git rev-parse --verify refs/test/no-such-ref
'

# ---------------------------------------------------------------------------
# update-ref with reflog message (-m)
# ---------------------------------------------------------------------------
test_expect_success 'update-ref -m accepts a message' '
	cd repo &&
	git update-ref -m "test message" refs/test/msg-ref $(cat ../first) &&
	result=$(git rev-parse refs/test/msg-ref) &&
	test "$result" = "$(cat ../first)"
'

# ---------------------------------------------------------------------------
# update-ref --stdin
# ---------------------------------------------------------------------------
test_expect_success 'update-ref --stdin processes update command' '
	cd repo &&
	echo "update refs/test/stdin-ref $(cat ../second)" |
	git update-ref --stdin &&
	result=$(git rev-parse refs/test/stdin-ref) &&
	test "$result" = "$(cat ../second)"
'

test_expect_success 'update-ref --stdin processes delete command' '
	cd repo &&
	echo "delete refs/test/stdin-ref" |
	git update-ref --stdin &&
	test_must_fail git rev-parse --verify refs/test/stdin-ref
'

test_expect_success 'update-ref --stdin processes create command' '
	cd repo &&
	echo "create refs/test/stdin-create $(cat ../third)" |
	git update-ref --stdin &&
	result=$(git rev-parse refs/test/stdin-create) &&
	test "$result" = "$(cat ../third)"
'

# ---------------------------------------------------------------------------
# symbolic-ref
# ---------------------------------------------------------------------------
test_expect_success 'symbolic-ref shows HEAD target' '
	cd repo &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/master"
'

test_expect_success 'symbolic-ref can update HEAD' '
	cd repo &&
	git checkout -b test-branch &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/test-branch" &&
	git checkout master
'

test_expect_success 'symbolic-ref fails on non-symbolic ref' '
	cd repo &&
	test_must_fail git symbolic-ref refs/test/new-ref
'

# ---------------------------------------------------------------------------
# HEAD tracking through operations
# ---------------------------------------------------------------------------
test_expect_success 'HEAD follows branch after commit' '
	cd repo &&
	before=$(git rev-parse HEAD) &&
	echo "new" >new-file.txt &&
	git add new-file.txt &&
	git commit -m "new commit" &&
	after=$(git rev-parse HEAD) &&
	test "$before" != "$after"
'

test_expect_success 'HEAD follows branch after reset --hard' '
	cd repo &&
	git reset --hard $(cat ../third) &&
	result=$(git rev-parse HEAD) &&
	test "$result" = "$(cat ../third)"
'

test_expect_success 'HEAD follows branch after reset --soft' '
	cd repo &&
	git reset --soft $(cat ../second) &&
	result=$(git rev-parse HEAD) &&
	test "$result" = "$(cat ../second)"
'

# ---------------------------------------------------------------------------
# show-ref for updated refs
# ---------------------------------------------------------------------------
test_expect_success 'show-ref lists custom refs' '
	cd repo &&
	git show-ref >../refs.out &&
	grep "refs/test/new-ref" ../refs.out &&
	grep "refs/test/msg-ref" ../refs.out
'

test_expect_success 'show-ref --verify checks specific ref' '
	cd repo &&
	git show-ref --verify refs/test/new-ref
'

test_expect_success 'show-ref --verify fails for missing ref' '
	cd repo &&
	test_must_fail git show-ref --verify refs/test/missing
'

# ---------------------------------------------------------------------------
# update-ref --no-deref
# ---------------------------------------------------------------------------
test_expect_success 'update-ref --no-deref on symbolic ref' '
	cd repo &&
	git symbolic-ref refs/test/sym-ref refs/heads/master &&
	git update-ref --no-deref refs/test/sym-ref $(cat ../first) &&
	result=$(git rev-parse refs/test/sym-ref) &&
	test "$result" = "$(cat ../first)"
'

# ---------------------------------------------------------------------------
# Branch operations affect refs
# ---------------------------------------------------------------------------
test_expect_success 'branch create adds ref' '
	cd repo &&
	git branch ref-test $(cat ../first) &&
	git show-ref --verify refs/heads/ref-test &&
	result=$(git rev-parse refs/heads/ref-test) &&
	test "$result" = "$(cat ../first)"
'

test_expect_success 'branch delete removes ref' '
	cd repo &&
	git branch -d ref-test &&
	test_must_fail git show-ref --verify refs/heads/ref-test
'

test_done
