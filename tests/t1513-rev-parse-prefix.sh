#!/bin/sh
# Ported subset from git/t/t1513-rev-parse-prefix.sh.

test_description='gust rev-parse --prefix subset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository state' '
	gust init repo &&
	cd repo &&
	echo "ref: refs/heads/main" >.git/HEAD &&
	mkdir -p sub1/sub2 &&
	echo top >top &&
	echo file1 >sub1/file1 &&
	echo file2 >sub1/sub2/file2 &&
	gust hash-object -w top >/dev/null &&
	gust hash-object -w sub1/file1 >/dev/null &&
	gust hash-object -w sub1/sub2/file2 >/dev/null &&
	gust update-index --add top sub1/file1 sub1/sub2/file2 &&
	tree=$(gust write-tree) &&
	commit=$(printf "one\n" | gust commit-tree "$tree") &&
	gust update-ref refs/heads/main "$commit"
'

test_expect_success 'empty prefix -- file' '
	cd repo &&
	gust rev-parse --prefix "" -- top sub1/file1 >actual &&
	cat >expect <<-\EOF &&
	--
	top
	sub1/file1
	EOF
	test_cmp expect actual
'

test_expect_success 'valid prefix -- file' '
	cd repo &&
	gust rev-parse --prefix sub1/ -- file1 sub2/file2 >actual &&
	cat >expect <<-\EOF &&
	--
	sub1/file1
	sub1/sub2/file2
	EOF
	test_cmp expect actual
'

test_expect_success 'valid prefix -- ../file keeps lexical parent' '
	cd repo &&
	gust rev-parse --prefix sub1/ -- ../top sub2/file2 >actual &&
	cat >expect <<-\EOF &&
	--
	sub1/../top
	sub1/sub2/file2
	EOF
	test_cmp expect actual
'

test_expect_success 'empty prefix HEAD:./path' '
	cd repo &&
	gust rev-parse --prefix "" HEAD:./top >actual &&
	gust rev-parse HEAD:top >expected &&
	test_cmp expected actual
'

test_expect_success 'valid prefix HEAD:./path' '
	cd repo &&
	gust rev-parse --prefix sub1/ HEAD:./file1 >actual &&
	gust rev-parse HEAD:sub1/file1 >expected &&
	test_cmp expected actual
'

test_expect_success 'valid prefix HEAD:../path' '
	cd repo &&
	gust rev-parse --prefix sub1/ HEAD:../top >actual &&
	gust rev-parse HEAD:top >expected &&
	test_cmp expected actual
'

test_expect_success 'prefix ignored with HEAD:top' '
	cd repo &&
	gust rev-parse --prefix sub1/ HEAD:top >actual &&
	gust rev-parse HEAD:top >expected &&
	test_cmp expected actual
'

test_expect_success 'disambiguate path with valid prefix' '
	cd repo &&
	gust rev-parse --prefix sub1/ file1 >actual &&
	echo sub1/file1 >expect &&
	test_cmp expect actual
'

test_expect_success 'file and refs with prefix' '
	cd repo &&
	gust rev-parse --prefix sub1/ main file1 >actual &&
	cat >expected <<-EOF &&
	$(gust rev-parse main)
	sub1/file1
	EOF
	test_cmp expected actual
'

test_expect_success 'two-level prefix with -- file' '
	cd repo &&
	gust rev-parse --prefix sub1/sub2/ -- file2 >actual &&
	cat >expect <<-\EOF &&
	--
	sub1/sub2/file2
	EOF
	test_cmp expect actual
'

test_done
