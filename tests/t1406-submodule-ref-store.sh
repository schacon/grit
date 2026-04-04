#!/bin/sh

test_description='test submodule ref store api'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# The upstream test uses test-tool ref-store which is a C test helper
# not available in grit. Instead, test submodule ref operations via
# normal git commands.

test_expect_success 'setup submodule' '
	git init sub &&
	(
		cd sub &&
		git commit --allow-empty -m "first" &&
		git checkout -b new-main &&
		git tag -a -m new-tag new-tag HEAD
	)
'

test_expect_success 'submodule refs are accessible' '
	git -C sub show-ref >actual &&
	grep refs/heads/main actual &&
	grep refs/heads/new-main actual &&
	grep refs/tags/new-tag actual
'

test_expect_success 'submodule pack-refs works' '
	git -C sub pack-refs --all &&
	git -C sub show-ref >actual &&
	grep refs/heads/main actual
'

test_done
