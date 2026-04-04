#!/bin/sh

test_description='credential helper tests'

. ./test-lib.sh

# External credential helper tests in upstream require GIT_TEST_CREDENTIAL_HELPER.
# Test the built-in credential plumbing that grit supports.

test_expect_success 'setup' '
	git init
'

test_expect_success 'credential fill provides protocol and host back' '
	printf "protocol=https\nhost=example.com\n\n" |
		git credential fill >actual 2>err || true &&
	# Even without a helper, fill should echo back known fields
	grep "protocol=https" actual &&
	grep "host=example.com" actual
'

test_done
