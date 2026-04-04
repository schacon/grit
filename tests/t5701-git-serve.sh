#!/bin/sh
# Ported from git/t/t5701-git-serve.sh
# Tests for git-serve (protocol v2 capability advertisement)
#
# These tests require the git-serve / upload-pack --advertise-refs
# server-side machinery which grit does not yet implement.

test_description='git-serve / protocol v2 capability advertisement'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'advertise capabilities via git-serve' '
	test_create_repo server &&
	git serve --advertise-capabilities 2>caps &&
	grep "version 2" caps
'

test_expect_failure 'ls-refs capability' '
	git serve --advertise-capabilities 2>caps &&
	grep "ls-refs" caps
'

test_expect_failure 'fetch capability' '
	git serve --advertise-capabilities 2>caps &&
	grep "fetch" caps
'

test_done
