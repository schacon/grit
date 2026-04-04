#!/bin/sh
# Ported from git/t/t5710-promisor-remote-capability.sh
# Tests for promisor-remote capability advertisement
#
# Requires partial clone / promisor remote support. Stubbed.

test_description='promisor-remote capability'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone with promisor remote and partial clone filter' '
	test_create_repo server &&
	(cd server && test_commit one) &&
	git -c protocol.version=2 clone --filter=blob:none server client &&
	# verify blobs are actually missing (promisor behavior)
	git -C client rev-list --objects --missing=print HEAD >objects &&
	grep "^?" objects
'

test_expect_failure 'fetch from promisor remote with lazy blob fetch' '
	false
'

test_done
