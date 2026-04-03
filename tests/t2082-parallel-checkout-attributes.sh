#!/bin/sh

test_description='parallel-checkout: attributes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Test basic checkout with .gitattributes

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo "content" >file.txt &&
	echo "*.txt text" >.gitattributes &&
	git add .gitattributes file.txt &&
	git commit -m "initial with attributes"
'

test_expect_success 'ls-files shows tracked files' '
	cd repo &&
	git ls-files >actual &&
	grep ".gitattributes" actual &&
	grep "file.txt" actual
'

test_expect_success 'checkout to new branch preserves attributes' '
	cd repo &&
	git checkout -b other &&
	test -f .gitattributes &&
	test -f file.txt
'

test_done
