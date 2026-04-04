#!/bin/sh
# Ported from git/t/t5750-bundle-uri-parse.sh
# Tests for bundle-uri configuration parsing
#
# Upstream tests use test-tool bundle-uri which is a C test helper.
# Grit does not have an equivalent test helper. Stubbed.

test_description='bundle-uri parse tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'bundle_uri_parse_line() just URIs' '
	false
'

test_expect_failure 'bundle_uri_parse_line(): relative URIs' '
	false
'

test_expect_failure 'parse config format: just URIs' '
	false
'

test_expect_failure 'parse config format: creationToken heuristic' '
	false
'

test_done
