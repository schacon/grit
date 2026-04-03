#!/bin/sh

test_description='test conversion filters on large files'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support clean/smudge filters, ident expansion,
# or read-tree --empty. Most conversion tests are expected failures.

test_expect_success 'setup' '
	git init &&
	git config core.bigfilethreshold 20
'

test_expect_success 'autocrlf=true converts on input (large file)' '
	printf "\$Id: foo\$\\r\\n" >small &&
	cat small small >large &&
	git config core.autocrlf true &&
	git read-tree --empty &&
	git add small large &&
	git cat-file blob :small >small.index &&
	git cat-file blob :large | head -n 1 >large.index &&
	test_cmp small.index large.index
'

test_expect_success 'eol=crlf converts on input (large file)' '
	echo "* eol=crlf" >.gitattributes &&
	printf "\$Id: foo\$\\r\\n" >small &&
	cat small small >large &&
	git read-tree --empty &&
	git add small large &&
	git cat-file blob :small >small.index &&
	git cat-file blob :large | head -n 1 >large.index &&
	test_cmp small.index large.index
'

test_expect_success 'user-defined filter converts on input (large file)' '
	git config filter.test.clean "sed s/.*/CLEAN/" &&
	echo "* filter=test" >.gitattributes &&
	printf "\$Id: foo\$\\r\\n" >small &&
	cat small small >large &&
	git read-tree --empty &&
	git add small large &&
	git cat-file blob :small >small.index &&
	git cat-file blob :large | head -n 1 >large.index &&
	test_cmp small.index large.index
'

test_done
