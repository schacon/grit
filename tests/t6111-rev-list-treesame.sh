#!/bin/sh

test_description='TREESAME and limiting with rev-list'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup linear history' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo initial >file && git add file &&
	test_tick && git commit -m A &&
	git tag A &&
	echo changed >file && git add file &&
	test_tick && git commit -m B &&
	git tag B &&
	echo changed2 >file && git add file &&
	test_tick && git commit -m C &&
	git tag C
'

test_expect_success 'rev-list linear history' '
	cd repo &&
	git rev-list HEAD >output &&
	test_line_count = 3 output
'

test_expect_success 'rev-list with range' '
	cd repo &&
	git rev-list A..C >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list --first-parent' '
	cd repo &&
	git rev-list --first-parent HEAD >output &&
	test_line_count = 3 output
'

test_expect_success 'setup merge history' '
	cd repo &&
	git checkout -b side A &&
	echo side >other && git add other &&
	test_tick && git commit -m D &&
	git tag D &&
	git checkout main &&
	git merge -m "merge" side &&
	git tag M
'

test_expect_success 'rev-list with merge' '
	cd repo &&
	git rev-list HEAD >output &&
	test_line_count = 5 output
'

test_expect_success 'rev-list --first-parent with merge' '
	cd repo &&
	git rev-list --first-parent HEAD >output &&
	test_line_count = 4 output
'

test_done
