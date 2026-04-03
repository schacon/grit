#!/bin/sh

test_description='clone from partial (not supported in grit)'

. ./test-lib.sh

# grit does not support --filter for clone.

test_expect_failure 'clone from partial not supported' '
	git init server &&
	(cd server && git config user.name T && git config user.email t@t && test_commit one) &&
	git clone --filter=blob:none --no-local --no-checkout "file://$(pwd)/server" partial
'

test_done
