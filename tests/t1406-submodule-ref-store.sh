#!/bin/sh

test_description='test submodule ref store api'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# Submodule ref store tests require test-tool ref-store
# which is not available in grit.

test_expect_success 'setup' '
	git init sub &&
	(
		cd sub &&
		git commit --allow-empty -m "first" &&
		git checkout -b new-main &&
		git tag -a -m new-tag new-tag HEAD
	)
'

test_expect_failure 'submodule ref-store tests (requires test-tool ref-store)' '
	test-tool ref-store submodule:sub pack-refs 3
'

test_done
