#!/bin/sh
# Ported subset from git/t/t1003-read-tree-prefix.sh.

test_description='grit read-tree --prefix'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup source tree' '
	grit init repo &&
	cd repo &&
	echo hello >one &&
	grit update-index --add one &&
	tree=$(grit write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'read-tree --prefix stages entries under prefix' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree "$(cat ../tree_oid)" &&
	grit read-tree --prefix=two/ "$(cat ../tree_oid)" &&
	grit ls-files >actual &&
	cat >expect <<-\EOF &&
	one
	two/one
	EOF
	test_cmp expect actual
'

test_expect_success 'read-tree --prefix rejects leading slash' '
	cd repo &&
	test_must_fail grit read-tree --prefix=/two/ "$(cat ../tree_oid)"
'

test_done
