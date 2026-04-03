#!/bin/sh

test_description='test sha1 collision detection'

. ./test-lib.sh

# These tests require test-tool sha1-is-sha1dc which is not available in grit.
# Grit uses Rust SHA1 implementation.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'sha1 collision detection (requires test-tool)' '
	test-tool sha1-is-sha1dc
'

test_done
