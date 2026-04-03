#!/bin/sh

test_description='Test reference backend URIs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not support extensions.refStorage or GIT_REFERENCE_BACKEND.
# All tests are expected failures.

test_expect_success 'setup basic repo' '
	git init repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	)
'

test_expect_failure 'refs list via alternate backend URI (config)' '
	(
		cd repo &&
		git config set extensions.refStorage "files" &&
		git refs list >actual &&
		test_line_count -gt 0 actual
	)
'

test_expect_failure 'refs list via alternate backend URI (env)' '
	(
		cd repo &&
		GIT_REFERENCE_BACKEND="files" git refs list >actual &&
		test_line_count -gt 0 actual
	)
'

test_done
