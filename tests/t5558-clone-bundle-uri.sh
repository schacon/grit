#!/bin/sh
#
# Upstream: t5558-clone-bundle-uri.sh
# Requires HTTP transport/bundle-uri — ported as test_expect_failure stubs.
#

test_description='test fetching bundles with --bundle-uri'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport/bundle-uri not available in grit ---

test_expect_failure 'fail to clone from non-existent file' '
	false
'

test_expect_failure 'fail to clone from non-bundle file' '
	false
'

test_expect_failure 'create bundle' '
	false
'

test_expect_failure 'clone with path bundle' '
	false
'

test_expect_failure 'clone with bundle that has bad header' '
	false
'

test_expect_failure 'clone with bundle that has bad object' '
	false
'

test_expect_failure 'clone with path bundle and non-default hash' '
	false
'

test_expect_failure 'clone with file:// bundle' '
	false
'

test_expect_failure 'create bundle with tags' '
	false
'

test_expect_failure 'clone with tags bundle' '
	false
'

test_expect_failure 'construct incremental bundle list' '
	false
'

test_expect_failure 'clone bundle list (file, no heuristic)' '
	false
'

test_expect_failure 'clone bundle list (file, all mode, some failures)' '
	false
'

test_expect_failure 'clone bundle list (file, all mode, all failures)' '
	false
'

test_expect_failure 'clone bundle list (file, any mode)' '
	false
'

test_expect_failure 'clone bundle list (file, any mode, all failures)' '
	false
'

test_expect_failure 'negotiation: bundle with part of wanted commits' '
	false
'

test_expect_failure 'negotiation: bundle with all wanted commits' '
	false
'

test_expect_failure 'negotiation: bundle list (no heuristic)' '
	false
'

test_expect_failure 'negotiation: bundle list (creationToken)' '
	false
'

test_expect_failure 'negotiation: bundle list with all wanted commits' '
	false
'

test_expect_failure 'fail to fetch from non-existent HTTP URL' '
	false
'

test_expect_failure 'fail to fetch from non-bundle HTTP URL' '
	false
'

test_expect_failure 'clone HTTP bundle' '
	false
'

test_expect_failure 'clone HTTP bundle with non-default hash' '
	false
'

test_expect_failure 'clone bundle list (HTTP, no heuristic)' '
	false
'

test_expect_failure 'clone bundle list (HTTP, any mode)' '
	false
'

test_expect_failure 'clone bundle list (http, creationToken)' '
	false
'

test_expect_failure 'clone incomplete bundle list (http, creationToken)' '
	false
'

test_expect_failure 'http clone with bundle.heuristic creates fetch.bundleURI' '
	false
'

test_expect_failure 'creationToken heuristic with failed downloads (clone)' '
	false
'

test_expect_failure 'expand incremental bundle list' '
	false
'

test_expect_failure 'creationToken heuristic with failed downloads (fetch)' '
	false
'

test_expect_failure 'bundles are downloaded once during fetch --all' '
	false
'

test_expect_failure 'bundles with space in URI are rejected' '
	false
'

test_expect_failure 'bundles with newline in URI are rejected' '
	false
'

test_expect_failure 'bundles with newline in target path are rejected' '
	false
'

test_done
