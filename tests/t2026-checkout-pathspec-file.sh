#!/bin/sh

test_description='checkout with pathspec operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo 1 >fileA.t &&
	echo 1 >fileB.t &&
	echo 1 >fileC.t &&
	git add fileA.t fileB.t fileC.t &&
	git commit -m "files 1" &&

	echo 2 >fileA.t &&
	echo 2 >fileB.t &&
	echo 2 >fileC.t &&
	git add fileA.t fileB.t fileC.t &&
	git commit -m "files 2" &&
	git tag checkpoint
'

test_expect_success 'checkout HEAD~1 -- <path> restores single file' '
	git reset --hard checkpoint &&
	git checkout HEAD~1 -- fileA.t &&
	echo 1 >expect &&
	test_cmp expect fileA.t &&
	echo 2 >expect &&
	test_cmp expect fileB.t
'

test_expect_success 'checkout HEAD~1 -- multiple paths restores them' '
	git reset --hard checkpoint &&
	git checkout HEAD~1 -- fileA.t fileB.t &&
	echo 1 >expect &&
	test_cmp expect fileA.t &&
	test_cmp expect fileB.t &&
	echo 2 >expect &&
	test_cmp expect fileC.t
'

test_done
