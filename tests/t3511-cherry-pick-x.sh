#!/bin/sh
# Ported from git/t/t3511-cherry-pick-x.sh
# Tests for cherry-pick -x and -s

test_description='Test cherry-pick -x and -s'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo unrelated >unrelated &&
	git add unrelated &&
	echo a >foo &&
	git add foo &&
	test_tick &&
	git commit -m "initial" &&
	git tag initial &&

	echo b >foo &&
	git add foo &&
	test_tick &&
	git commit -m "base: commit message" &&
	git tag mesg-one-line
'

test_expect_success 'cherry-pick -x appends cherry-pick reference' '
	git checkout initial^0 &&
	sha1=$(git rev-parse mesg-one-line^0) &&
	git cherry-pick -x mesg-one-line &&
	git log -n1 --format=%B >actual &&
	grep "cherry picked from commit $sha1" actual
'

test_expect_success 'cherry-pick -s appends signed-off-by' '
	git checkout initial^0 &&
	git cherry-pick -s mesg-one-line &&
	git log -n1 --format=%B >actual &&
	grep "Signed-off-by: $GIT_COMMITTER_NAME <$GIT_COMMITTER_EMAIL>" actual
'

test_expect_success 'cherry-pick -x preserves subject' '
	git checkout initial^0 &&
	git cherry-pick -x mesg-one-line &&
	git log -n1 --format=%s >actual &&
	echo "base: commit message" >expect &&
	test_cmp expect actual
'

test_expect_success 'cherry-pick -s preserves subject' '
	git checkout initial^0 &&
	git cherry-pick -s mesg-one-line &&
	git log -n1 --format=%s >actual &&
	echo "base: commit message" >expect &&
	test_cmp expect actual
'

test_expect_success 'cherry-pick -x with multi-line message preserves body' '
	git checkout initial^0 &&
	echo c >foo &&
	git add foo &&
	test_tick &&
	git commit -m "subject line

body text here" &&
	git tag mesg-multi &&

	git checkout initial^0 &&
	sha1=$(git rev-parse mesg-multi^0) &&
	git cherry-pick -x mesg-multi &&
	git log -n1 --format=%B >actual &&
	grep "subject line" actual &&
	grep "body text here" actual &&
	grep "cherry picked from commit $sha1" actual
'

test_expect_success 'cherry-pick without -x does not add reference' '
	git checkout initial^0 &&
	git cherry-pick mesg-one-line &&
	git log -n1 --format=%B >actual &&
	! grep "cherry picked from" actual
'

test_expect_success 'cherry-pick -x -s adds both reference and signoff' '
	git checkout initial^0 &&
	sha1=$(git rev-parse mesg-one-line^0) &&
	git cherry-pick -x -s mesg-one-line &&
	git log -n1 --format=%B >actual &&
	grep "cherry picked from commit $sha1" actual &&
	grep "Signed-off-by:" actual
'

test_done
