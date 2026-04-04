#!/bin/sh

test_description='reftable HTTPD tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not have httpd test infrastructure.
# All httpd-dependent tests are skipped.

test_expect_success 'serving ls-remote via HTTP with reftable' '
	false
'

test_done
