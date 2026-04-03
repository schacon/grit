#!/bin/sh
test_description='git grep in binary files'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "text pattern here" >textfile &&
	printf "binary\0content pattern\0here" >binaryfile &&
	git add . &&
	git commit -m "initial"
'

test_expect_success 'grep finds pattern in text file' '
	cd repo &&
	git grep "pattern" textfile
'

test_expect_success 'grep reports binary file matches' '
	cd repo &&
	git grep "pattern" >actual &&
	grep "textfile" actual
'

test_done
