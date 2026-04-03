#!/bin/sh

test_description='test handling of bogus index entries'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'create tree with null sha1' '
	cd repo &&
	tree=$(printf "160000 commit $ZERO_OID\\tbroken\\n" | git mktree) &&
	test -n "$tree"
'

test_done
