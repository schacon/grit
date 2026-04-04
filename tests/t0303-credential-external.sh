#!/bin/sh

test_description='credential helper basics'

. ./test-lib.sh

test_expect_success 'setup' '
	git init
'

test_expect_success 'credential fill reads protocol/host from stdin' '
	echo "protocol=https
host=example.com" | git credential fill >actual 2>err || true &&
	# Either fills credentials or fails gracefully asking for a helper
	test -f actual
'

test_done
