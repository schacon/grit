#!/bin/sh
# Ported from git/t/t3102-ls-tree-wildcards.sh (harness-compatible subset).

test_description='grit ls-tree globs and literal paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	grit init repo &&
	cd repo &&
	mkdir -p a aa "a[a]" &&
	: >a/one &&
	: >aa/two &&
	: >"a[a]/three" &&
	grit update-index --add a/one aa/two "a[a]/three" &&
	tree=$(grit write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'ls-tree a[a] matches literally' '
	cd repo &&
	empty_blob=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 &&
	cat >expect <<-EOF &&
	100644 blob $empty_blob	a[a]/three
	EOF
	grit ls-tree -r "$(cat ../tree_oid)" "a[a]" >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-tree a matches literally and not as glob' '
	cd repo &&
	empty_blob=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 &&
	cat >expect <<-EOF &&
	100644 blob $empty_blob	a/one
	EOF
	grit ls-tree -r "$(cat ../tree_oid)" a >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-tree aa matches aa directory' '
	cd repo &&
	empty_blob=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 &&
	cat >expect <<-EOF &&
	100644 blob $empty_blob	aa/two
	EOF
	grit ls-tree -r "$(cat ../tree_oid)" aa >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-tree with nonexistent path produces empty output' '
	cd repo &&
	grit ls-tree -r "$(cat ../tree_oid)" "nonexistent" >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-tree with multiple path args' '
	cd repo &&
	empty_blob=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391 &&
	cat >expect <<-EOF &&
	100644 blob $empty_blob	a/one
	100644 blob $empty_blob	aa/two
	EOF
	grit ls-tree -r "$(cat ../tree_oid)" a aa >actual &&
	test_cmp expect actual
'

test_done
