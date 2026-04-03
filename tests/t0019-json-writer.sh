#!/bin/sh

test_description='test json-writer JSON generation'

. ./test-lib.sh

# These tests require test-tool json-writer which is not available in grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'json-writer unit tests (requires test-tool)' '
	test-tool json-writer -u
'

test_done
