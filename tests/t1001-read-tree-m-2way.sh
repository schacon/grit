#!/bin/sh
# Ported subset from git/t/t1001-read-tree-m-2way.sh.

test_description='gust read-tree -m two-way carry-forward'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup baseline and merged trees' '
	gust init repo &&
	cd repo &&
	echo base-bozbar >bozbar &&
	echo nitfol >nitfol &&
	echo rezrov >rezrov &&
	rm -f .git/index &&
	gust update-index --add bozbar nitfol rezrov &&
	tree_h=$(gust write-tree) &&
	echo "$tree_h" >../tree_h &&
	echo merged-bozbar >bozbar &&
	echo frotz >frotz &&
	rm -f .git/index &&
	gust update-index --add bozbar frotz nitfol &&
	tree_m=$(gust write-tree) &&
	echo "$tree_m" >../tree_m &&
	gust ls-files >expect_m
'

test_expect_success 'empty index merged from H to M equals M' '
	cd repo &&
	rm -f .git/index &&
	gust read-tree -m "$(cat ../tree_h)" "$(cat ../tree_m)" &&
	gust ls-files >actual &&
	test_cmp expect_m actual
'

test_expect_success 'carry-forward local addition keeps extra path' '
	cd repo &&
	rm -f .git/index &&
	gust read-tree "$(cat ../tree_h)" &&
	echo yomin >yomin &&
	gust update-index --add yomin &&
	gust read-tree -m "$(cat ../tree_h)" "$(cat ../tree_m)" &&
	gust ls-files >actual &&
	cat >expect <<-\EOF &&
	bozbar
	frotz
	nitfol
	yomin
	EOF
	test_cmp expect actual
'

test_expect_success 'conflicting local change aborts two-way merge' '
	cd repo &&
	rm -f .git/index &&
	gust read-tree "$(cat ../tree_h)" &&
	echo conflict >bozbar &&
	gust update-index --add bozbar &&
	test_must_fail gust read-tree -m "$(cat ../tree_h)" "$(cat ../tree_m)"
'

test_done
