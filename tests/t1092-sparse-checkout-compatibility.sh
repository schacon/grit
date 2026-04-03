#!/bin/sh

test_description='compare full workdir to sparse workdir'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	(
		cd repo &&
		echo a >a &&
		mkdir folder1 folder2 deep &&
		mkdir deep/deeper1 deep/deeper2 &&
		cp a folder1 &&
		cp a folder2 &&
		cp a deep &&
		cp a deep/deeper1 &&
		cp a deep/deeper2 &&
		git add . &&
		git commit -m "initial"
	)
'

test_expect_success 'sparse-checkout set and list' '
	(
		cd repo &&
		git sparse-checkout init &&
		git sparse-checkout set folder1 deep/deeper1 &&
		git sparse-checkout list >actual &&
		grep "folder1" actual &&
		grep "deep/deeper1" actual
	)
'

test_expect_failure 'sparse-checkout limits working tree' '
	(
		cd repo &&
		git sparse-checkout set folder1 &&
		test_path_is_file a &&
		test_path_is_file folder1/a &&
		test_path_is_missing folder2/a
	)
'

test_expect_success 'sparse-checkout disable restores all files' '
	(
		cd repo &&
		git sparse-checkout disable &&
		test_path_is_file a &&
		test_path_is_file folder1/a &&
		test_path_is_file folder2/a &&
		test_path_is_file deep/a
	)
'

test_done
