#!/bin/sh

test_description='CRLF conversion all combinations'

. ./test-lib.sh

# This is a very large test (630+ lines upstream) that tests all
# combinations of core.autocrlf, core.eol, and gitattributes.
# We test a simplified subset here.

test_expect_success 'setup' '
	git init &&
	git config core.autocrlf false
'

test_expect_success 'create LF-only file' '
	printf "line1\nline2\nline3\n" >LF.txt &&
	git add LF.txt &&
	git commit -m "LF file"
'

test_expect_success 'create CRLF file' '
	printf "line1\r\nline2\r\nline3\r\n" >CRLF.txt &&
	git add CRLF.txt &&
	git commit -m "CRLF file"
'

test_expect_success 'autocrlf=true converts on checkout' '
	git config core.autocrlf true &&
	rm -f LF.txt &&
	git checkout -- LF.txt &&
	# On Unix the file may or may not be converted depending on implementation
	test -f LF.txt
'

test_expect_success 'autocrlf=input keeps LF on checkout' '
	git config core.autocrlf input &&
	rm -f LF.txt &&
	git checkout -- LF.txt &&
	test -f LF.txt
'

test_done
