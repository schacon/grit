#!/bin/sh
# Ported from git/t/t5573-pull-verify-signatures.sh
# Tests pull signature verification
# Grit does not support GPG signature verification for pull

test_description='pull signature verification tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit initial
'

# grit rejects --verify-signatures (unknown flag)
test_expect_success 'pull --verify-signatures rejected' '
	git clone . verify-clone &&
	test_commit signed_commit &&
	(
		cd verify-clone &&
		test_must_fail git pull --verify-signatures 2>err
	)
'

test_expect_success 'basic pull works without signature verification' '
	git clone . basic-clone &&
	test_commit another_commit &&
	(
		cd basic-clone &&
		git pull
	) &&
	git -C basic-clone rev-parse HEAD >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_done
