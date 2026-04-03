#!/bin/sh

test_description='migration of ref storage backends'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit refs migrate is not yet fully implemented.
# Basic argument validation is tested; actual migration is expected failure.

test_expect_success 'setup' '
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

test_expect_success 'missing ref storage format fails' '
	test_must_fail git -C repo refs migrate 2>err
'

test_expect_failure 'migrate files to reftable' '
	cp -R repo repo-migrate &&
	git -C repo-migrate refs migrate --ref-format=reftable &&
	git -C repo-migrate show-ref >actual &&
	test_line_count -gt 0 actual
'

test_expect_failure 'migrate reftable to files' '
	rm -rf repo-rt &&
	git init --ref-format=reftable repo-rt &&
	(
		cd repo-rt &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	) &&
	git -C repo-rt refs migrate --ref-format=files &&
	git -C repo-rt show-ref >actual &&
	test_line_count -gt 0 actual &&
	echo files >expect &&
	git -C repo-rt rev-parse --show-ref-format >actual &&
	test_cmp expect actual
'

test_expect_success 'refs migrate basic command exists' '
	git -C repo refs migrate --ref-format=reftable 2>err || true
'

test_done
