#!/bin/sh

test_description='git read-tree in partial clones (not supported in grit)'

. ./test-lib.sh

# grit does not support --filter for clone.

test_expect_failure 'partial clone not supported for read-tree test' '
	git init server &&
	(cd server && git config user.name T && git config user.email t@t) &&
	echo foo >server/one &&
	echo bar >server/two &&
	(cd server && git add one two && git commit -m "initial") &&
	git clone --bare --filter=blob:none "file://$(pwd)/server" client
'

test_done
