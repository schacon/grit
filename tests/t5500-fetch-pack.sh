#!/bin/sh
# Ported from git/t/t5500-fetch-pack.sh
# Simplified: tests basic fetch-pack/upload-pack protocol

test_description='Testing fetch-pack and upload-pack'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	git branch side &&
	echo more >file &&
	git add file &&
	git commit -m second
'

test_expect_success 'clone and verify' '
	git clone . cloned &&
	(
		cd cloned &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)" &&
		test "$(git rev-parse origin/side)" = "$(cd .. && git rev-parse side)"
	)
'

test_expect_success 'fetch after new commits' '
	echo even-more >file &&
	git add file &&
	git commit -m third &&
	(
		cd cloned &&
		git fetch origin &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_expect_success 'clone --bare' '
	git clone --bare . bare-clone.git &&
	git --git-dir=bare-clone.git rev-parse main >actual &&
	git rev-parse main >expect &&
	test_cmp expect actual
'

test_expect_success 'fetch with tags' '
	git tag test-tag &&
	(
		cd cloned &&
		git fetch --tags origin &&
		git show-ref test-tag
	)
'

test_done
