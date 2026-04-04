#!/bin/sh

test_description='reftable HTTPD tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='skipping HTTPD tests; httpd infrastructure not available in grit'
test_done
