#!/bin/sh

test_description='Testing the various Bloom filter computations'

. ./test-lib.sh

# These tests require test-tool bloom which is not available in grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'bloom filter tests (requires test-tool)' '
	test-tool bloom get_murmur3 "" >actual
'

test_done
