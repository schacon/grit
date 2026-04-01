#!/bin/sh
# Ported from git/t/t3102-ls-tree-wildcards.sh (harness-compatible subset).

test_description='gust ls-tree globs and literal paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	gust init repo &&
	cd repo &&
	mkdir -p a aa "a[a]" &&
	: >a/one &&
	: >aa/two &&
	: >"a[a]/three" &&
	gust update-index --add a/one aa/two "a[a]/three" &&
	tree=$(gust write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'ls-tree a[a] matches literally' '
	cd repo &&
	empty_blob=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 &&
	cat >expect <<-EOF &&
	100644 blob $empty_blob	a[a]/three
	EOF
	gust ls-tree -r "$(cat ../tree_oid)" "a[a]" >actual &&
	test_cmp expect actual
'

test_done
