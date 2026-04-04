#!/bin/sh

test_description='partial clone (not supported in grit)'

. ./test-lib.sh

# grit does not support --filter for clone.

test_expect_success 'partial clone with --filter' '
	git init server &&
	(cd server && git config user.name T && git config user.email t@t && test_commit one) &&
	git clone --filter="blob:none" "file://$(pwd)/server" client
'

test_done
