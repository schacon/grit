#!/bin/sh

test_description='restore with multiple pathspecs'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir dir1 &&
	echo 1 >dir1/file &&
	echo 1 >fileA.t &&
	echo 1 >fileB.t &&
	echo 1 >fileC.t &&
	git add dir1 fileA.t fileB.t fileC.t &&
	git commit -m "files 1" &&
	git tag v1 &&

	echo 2 >dir1/file &&
	echo 2 >fileA.t &&
	echo 2 >fileB.t &&
	echo 2 >fileC.t &&
	git add dir1 fileA.t fileB.t fileC.t &&
	git commit -m "files 2" &&
	git tag checkpoint
'

test_expect_success 'restore single file from source' '
	git reset --hard checkpoint &&
	git restore --source v1 fileA.t &&
	echo 1 >expect &&
	test_cmp expect fileA.t &&
	echo 2 >expect &&
	test_cmp expect fileB.t
'

test_expect_success 'restore multiple files from source' '
	git reset --hard checkpoint &&
	git restore --source v1 fileA.t fileB.t &&
	echo 1 >expect &&
	test_cmp expect fileA.t &&
	test_cmp expect fileB.t &&
	echo 2 >expect &&
	test_cmp expect fileC.t
'

test_expect_success 'restore --staged removes from index' '
	git reset --hard checkpoint &&
	echo changed >fileA.t &&
	git add fileA.t &&
	git restore --staged fileA.t &&
	git diff --cached --quiet HEAD
'

test_done
