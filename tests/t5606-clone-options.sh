#!/bin/sh
# Ported from git/t/t5606-clone-options.sh
# Tests for basic clone options

test_description='basic clone options'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	mkdir parent &&
	(cd parent && git init -q &&
	 git config user.email "test@example.com" &&
	 git config user.name "Test User" &&
	 echo one >file && git add file &&
	 git commit -m one)
'

test_expect_success 'clone uses "origin" for default remote name' '
	git clone parent clone-default-origin &&
	git -C clone-default-origin rev-parse --verify refs/remotes/origin/main
'

test_expect_success 'clone --bare' '
	git clone --bare parent clone-bare.git &&
	test -f clone-bare.git/HEAD &&
	test -d clone-bare.git/refs
'

test_expect_success 'clone --bare sets core.bare to true' '
	echo true >expect &&
	git -C clone-bare.git rev-parse --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success 'clone -q suppresses output' '
	git clone -q parent clone-quiet 2>err &&
	test_must_be_empty err
'

test_expect_success 'clone --branch checks out specific branch' '
	(cd parent && git checkout -b other &&
	 echo two >file && git commit -a -m two &&
	 git checkout main) &&
	git clone --branch other parent clone-branch &&
	echo two >expect &&
	cat clone-branch/file >actual &&
	test_cmp expect actual
'

test_expect_success 'clone --no-checkout' '
	git clone --no-checkout parent clone-no-checkout &&
	test_path_is_missing clone-no-checkout/file
'

test_done
