#!/bin/sh
# Ported from git/t/t3902-quoted.sh (ls-tree focused subset).

test_description='grit ls-tree quoted output'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

HT='	'
DQ='"'

test_expect_success 'setup repository with quote-sensitive names' '
	grit init repo &&
	cd repo &&
	echo initial >Name &&
	echo initial >"With SP in it" &&
	echo initial >"Name and an${HT}HT" &&
	echo initial >"Name${DQ}" &&
	grit update-index --add Name "With SP in it" "Name and an${HT}HT" "Name${DQ}" &&
	tree=$(grit write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'ls-tree --name-only -r defaults to quoted paths' '
	cd repo &&
	cat >expect <<-\EOF &&
	Name
	"Name and an\tHT"
	"Name\""
	With SP in it
	EOF
	grit ls-tree --name-only -r "$(cat ../tree_oid)" >actual &&
	test_cmp expect actual
'

test_done
