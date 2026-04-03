#!/bin/sh

test_description='git-merge with case-changing rename on case-insensitive file system'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# This test requires a case-insensitive filesystem.
# On Linux (case-sensitive), we skip all tests.
if ! test_have_prereq CASE_INSENSITIVE_FS
then
	# No tests to run on case-sensitive FS
	test_done
	exit 0
fi

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_done
