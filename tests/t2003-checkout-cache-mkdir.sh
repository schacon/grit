#!/bin/sh
# Ported subset from git/t/t2003-checkout-cache-mkdir.sh

test_description='grit checkout-index --mkdir/--prefix'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	mkdir path1 &&
	echo frotz >path0 &&
	echo rezrov >path1/file1 &&
	git update-index --add path0 path1/file1
'

test_expect_success 'checkout-index requires --mkdir for missing leading dirs' '
	cd repo &&
	rm -rf out &&
	test_must_fail git checkout-index --prefix=out/ path1/file1 2>stderr &&
	grep "leading directories do not exist" stderr
'

test_expect_success 'checkout-index --mkdir creates leading dirs with --prefix' '
	cd repo &&
	rm -rf out &&
	git checkout-index --mkdir --prefix=out/ -f -a &&
	test_path_is_file out/path0 &&
	test_path_is_file out/path1/file1 &&
	test_path_is_file path0 &&
	test_path_is_file path1/file1
'

test_expect_success 'use --prefix=path2/' '
	cd repo &&
	rm -fr path0 path1 path2 &&
	mkdir path2 &&
	git checkout-index --prefix=path2/ --mkdir -f -a &&
	test_path_is_file path2/path0 &&
	test_path_is_file path2/path1/file1 &&
	test_path_is_missing path0 &&
	test_path_is_missing path1/file1
'

test_expect_success 'use --prefix=tmp-' '
	cd repo &&
	rm -fr path0 path1 path2 tmp* &&
	git checkout-index --prefix=tmp- --mkdir -f -a &&
	test_path_is_file tmp-path0 &&
	test_path_is_file tmp-path1/file1 &&
	test_path_is_missing path0 &&
	test_path_is_missing path1/file1
'

test_expect_success 'use --prefix=tmp/orary/ where tmp is a symlink' '
	cd repo &&
	rm -fr path0 path1 path2 tmp* &&
	mkdir tmp1 tmp1/orary &&
	ln -s tmp1 tmp &&
	git checkout-index --prefix=tmp/orary/ --mkdir -f -a &&
	test -d tmp1/orary &&
	test_path_is_file tmp1/orary/path0 &&
	test_path_is_file tmp1/orary/path1/file1 &&
	test -L tmp
'

test_expect_success 'use --prefix=tmp/orary- where tmp is a symlink' '
	cd repo &&
	rm -fr path0 path1 path2 tmp* &&
	mkdir tmp1 &&
	ln -s tmp1 tmp &&
	git checkout-index --prefix=tmp/orary- --mkdir -f -a &&
	test_path_is_file tmp1/orary-path0 &&
	test_path_is_file tmp1/orary-path1/file1 &&
	test -L tmp
'

test_done
