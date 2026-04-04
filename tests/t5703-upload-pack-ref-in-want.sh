#!/bin/sh
# Ported from git/t/t5703-upload-pack-ref-in-want.sh
# Tests for upload-pack ref-in-want capability
#
# Requires upload-pack server support. Stubbed.

test_description='upload-pack ref-in-want'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'upload-pack advertises ref-in-want' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git upload-pack --advertise-refs server >refs &&
	grep "ref-in-want" refs
'

test_expect_failure 'fetch with ref-in-want' '
	git clone server client &&
	(cd server && test_commit two) &&
	git -C client fetch --negotiate-only origin refs/heads/main
'

test_done
