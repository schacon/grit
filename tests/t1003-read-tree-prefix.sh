#!/bin/sh
# Ported subset from git/t/t1003-read-tree-prefix.sh.

test_description='gust read-tree --prefix'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup source tree' '
	gust init repo &&
	cd repo &&
	echo hello >one &&
	gust update-index --add one &&
	tree=$(gust write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'read-tree --prefix stages entries under prefix' '
	cd repo &&
	rm -f .git/index &&
	gust read-tree "$(cat ../tree_oid)" &&
	gust read-tree --prefix=two/ "$(cat ../tree_oid)" &&
	gust ls-files >actual &&
	cat >expect <<-\EOF &&
	one
	two/one
	EOF
	test_cmp expect actual
'

test_expect_success 'read-tree --prefix rejects leading slash' '
	cd repo &&
	test_must_fail gust read-tree --prefix=/two/ "$(cat ../tree_oid)"
'

test_done
