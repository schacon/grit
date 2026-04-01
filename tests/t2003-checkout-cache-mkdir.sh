#!/bin/sh
# Ported subset from git/t/t2003-checkout-cache-mkdir.sh

test_description='gust checkout-index --mkdir/--prefix'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	gust init repo &&
	cd repo &&
	mkdir path1 &&
	echo frotz >path0 &&
	echo rezrov >path1/file1 &&
	gust update-index --add path0 path1/file1
'

test_expect_success 'checkout-index requires --mkdir for missing leading dirs' '
	cd repo &&
	rm -rf out &&
	test_must_fail gust checkout-index --prefix=out/ path1/file1 2>stderr &&
	grep "leading directories do not exist" stderr
'

test_expect_success 'checkout-index --mkdir creates leading dirs with --prefix' '
	cd repo &&
	rm -rf out &&
	gust checkout-index --mkdir --prefix=out/ -f -a &&
	test_path_is_file out/path0 &&
	test_path_is_file out/path1/file1 &&
	test_path_is_file path0 &&
	test_path_is_file path1/file1
'

test_done
