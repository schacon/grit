#!/bin/sh

test_description='stash push with pathspec'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo A >fileA.t &&
	echo B >fileB.t &&
	echo C >fileC.t &&
	git add fileA.t fileB.t fileC.t &&
	test_tick &&
	git commit -m "Files" &&
	git tag checkpoint
'

test_expect_success 'stash push with pathspec stashes only specified files' '
	git reset --hard checkpoint &&
	echo A2 >fileA.t &&
	echo B2 >fileB.t &&
	echo C2 >fileC.t &&
	git stash push -- fileA.t &&
	echo A >expect &&
	test_cmp expect fileA.t &&
	echo B2 >expect &&
	test_cmp expect fileB.t &&
	git reset --hard checkpoint &&
	git stash pop
'

test_expect_success 'stash push with multiple pathspecs' '
	git reset --hard checkpoint &&
	echo A2 >fileA.t &&
	echo B2 >fileB.t &&
	echo C2 >fileC.t &&
	git stash push -- fileA.t fileB.t &&
	echo A >expect &&
	test_cmp expect fileA.t &&
	echo B >expect &&
	test_cmp expect fileB.t &&
	echo C2 >expect &&
	test_cmp expect fileC.t &&
	git reset --hard checkpoint &&
	git stash pop
'

test_expect_success 'stash show lists changed files' '
	git reset --hard checkpoint &&
	git stash clear &&
	echo A2 >fileA.t &&
	git stash push -- fileA.t &&
	git stash show >actual &&
	grep "fileA.t" actual &&
	git reset --hard checkpoint &&
	git stash pop
'

test_done
