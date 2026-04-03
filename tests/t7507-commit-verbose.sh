#!/bin/sh
#
# t7507-commit-verbose.sh — commit -v (verbose) and related editor behavior
#
# Note: grit does not yet support commit -v. We test that it is properly
# rejected and verify related commit message behaviors that do work.
#

test_description='commit verbose mode and message handling'
. ./test-lib.sh

# ── setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: init repo with config' '
	git init verbose-repo &&
	cd verbose-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "initial" >file.txt &&
	git add file.txt &&
	git commit -m "initial commit"
'

# ── commit -v is not yet supported ──────────────────────────────────────────

test_expect_success 'commit -v is rejected (not yet implemented)' '
	cd verbose-repo &&
	echo "change1" >file.txt &&
	git add file.txt &&
	test_must_fail git commit -v -m "verbose attempt" 2>err &&
	grep -i "unexpected\|unrecognized\|unknown" err
'

# ── -m message ───────────────────────────────────────────────────────────────

test_expect_success 'commit -m creates commit with correct message' '
	cd verbose-repo &&
	echo "change2" >file.txt &&
	git add file.txt &&
	git commit -m "a specific message" &&
	git log --format="%s" -n 1 >actual &&
	echo "a specific message" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit -m with multi-word message' '
	cd verbose-repo &&
	echo "change3" >file.txt &&
	git add file.txt &&
	git commit -m "this is a longer commit message with spaces" &&
	git log --format="%s" -n 1 >actual &&
	echo "this is a longer commit message with spaces" >expect &&
	test_cmp expect actual
'

# ── -F file message ─────────────────────────────────────────────────────────

test_expect_success 'commit -F reads message from file' '
	cd verbose-repo &&
	echo "change4" >file.txt &&
	git add file.txt &&
	echo "message from file" >msg.txt &&
	git commit -F msg.txt &&
	git log --format="%s" -n 1 >actual &&
	echo "message from file" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit -F with multi-line message' '
	cd verbose-repo &&
	echo "change5" >file.txt &&
	git add file.txt &&
	printf "subject line\n\nbody paragraph" >msg.txt &&
	git commit -F msg.txt &&
	git log --format="%s" -n 1 >actual &&
	echo "subject line" >expect &&
	test_cmp expect actual &&
	git log --format="%b" -n 1 >actual-body &&
	grep "body paragraph" actual-body
'

# ── --allow-empty ────────────────────────────────────────────────────────────

test_expect_success 'commit --allow-empty works' '
	cd verbose-repo &&
	git commit --allow-empty -m "empty commit" &&
	git log --format="%s" -n 1 >actual &&
	echo "empty commit" >expect &&
	test_cmp expect actual
'

# ── --allow-empty-message ────────────────────────────────────────────────────

test_expect_success 'commit --allow-empty-message with empty -m succeeds' '
	cd verbose-repo &&
	echo "change6" >file.txt &&
	git add file.txt &&
	git commit --allow-empty-message -m ""
'

# ── --amend ──────────────────────────────────────────────────────────────────

test_expect_success 'commit --amend changes last commit message' '
	cd verbose-repo &&
	echo "change7" >file.txt &&
	git add file.txt &&
	git commit -m "original message" &&
	git commit --amend -m "amended message" &&
	git log --format="%s" -n 1 >actual &&
	echo "amended message" >expect &&
	test_cmp expect actual
'

# NOTE: grit --amend does not currently preserve the original author
test_expect_success 'commit --amend preserves author from amended commit' '
	cd verbose-repo &&
	echo "change8" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Original Author" \
	GIT_AUTHOR_EMAIL="orig@example.com" \
	git commit -m "by original author" &&
	git commit --amend -m "amended but same author" &&
	git log --format="%an <%ae>" -n 1 >actual &&
	echo "Original Author <orig@example.com>" >expect &&
	test_cmp expect actual
'

# ── -a (commit all tracked) ─────────────────────────────────────────────────

test_expect_success 'commit -a stages modified tracked files' '
	cd verbose-repo &&
	echo "change9" >file.txt &&
	git add file.txt &&
	git commit -m "baseline for -a test" &&
	echo "modified" >file.txt &&
	git commit -a -m "commit with -a" &&
	git log --format="%s" -n 1 >actual &&
	echo "commit with -a" >expect &&
	test_cmp expect actual
'

# ── -q (quiet) ───────────────────────────────────────────────────────────────

test_expect_success 'commit -q suppresses output' '
	cd verbose-repo &&
	echo "change10" >file.txt &&
	git add file.txt &&
	git commit -q -m "quiet commit" >stdout 2>&1 &&
	test_must_be_empty stdout
'

# ── -s (signoff) ────────────────────────────────────────────────────────────

# NOTE: grit -s/--signoff does not currently add the trailer
test_expect_success 'commit -s adds Signed-off-by trailer' '
	cd verbose-repo &&
	echo "change11" >file.txt &&
	git add file.txt &&
	git commit -s -m "signoff commit" &&
	git cat-file -p HEAD >commit-obj &&
	grep "Signed-off-by:" commit-obj
'

test_done
