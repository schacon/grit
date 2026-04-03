#!/bin/sh
# Ported from git/t/t5405-send-pack-rewind.sh
# Adapted: uses send-pack with bare repos

test_description='forced push to replace commit we do not have'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	>file1 && git add file1 && test_tick &&
	git commit -m Initial &&

	>file2 && git add file2 && test_tick &&
	git commit -m Second &&

	git clone --bare . dest.git &&

	git reset --hard HEAD^ &&
	>file3 && git add file3 && test_tick &&
	git commit -m Diverged
'

test_expect_success 'non forced push of diverged branch should fail' '
	test_must_fail git send-pack ./dest.git main:main
'

test_expect_success 'forced push of diverged branch should succeed' '
	git send-pack --force ./dest.git main:main
'

test_done
