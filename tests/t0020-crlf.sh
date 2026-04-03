#!/bin/sh

test_description='CRLF conversion'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

has_cr() {
	tr '\015' Q <"$1" | grep Q >/dev/null
}

test_expect_success 'setup' '
	mkdir repo &&
	cd repo &&
	git init &&
	git config core.autocrlf false &&
	test_write_lines Hello world how are you >one &&
	mkdir dir &&
	test_write_lines I am very very fine thank you >dir/two &&
	git add . &&
	git commit -m initial
'

test_expect_success 'checkout with autocrlf=input restores files without CR' '
	cd repo &&
	rm -f one dir/two &&
	git config core.autocrlf input &&
	git checkout -f HEAD &&
	test_path_is_file one &&
	test_path_is_file dir/two &&
	! has_cr one &&
	! has_cr dir/two
'

test_expect_success 'safecrlf: autocrlf=input, all CRLF' '
	cd repo &&
	git config core.autocrlf input &&
	git config core.safecrlf true &&
	printf "I am all CRLF\r\n" >allcrlf &&
	test_must_fail git add allcrlf
'

test_expect_success 'safecrlf: autocrlf=true, all LF' '
	cd repo &&
	git config core.autocrlf true &&
	git config core.safecrlf true &&
	test_write_lines I am all LF >alllf &&
	test_must_fail git add alllf
'

test_expect_success 'switch off autocrlf, safecrlf, reset HEAD' '
	cd repo &&
	git config core.autocrlf false &&
	git config core.safecrlf false &&
	git checkout -f HEAD
'

test_expect_success 'autocrlf false preserves LF' '
	cd repo &&
	git config core.autocrlf false &&
	rm -f one &&
	git checkout -f HEAD &&
	test_path_is_file one &&
	! has_cr one
'

test_expect_success 'autocrlf true adds CR on checkout' '
	cd repo &&
	git config core.autocrlf true &&
	rm -f one &&
	git checkout -f HEAD &&
	has_cr one
'

test_expect_success 'setting up for new autocrlf tests' '
	cd repo &&
	git config core.autocrlf false &&
	git config core.safecrlf false &&
	rm -rf .????* * &&
	test_write_lines I am all LF >alllf &&
	git add -A . &&
	git commit -m "alllf only"
'

test_expect_success 'report no change after setting autocrlf' '
	cd repo &&
	git config core.autocrlf true &&
	touch * &&
	git diff --exit-code
'

test_done
