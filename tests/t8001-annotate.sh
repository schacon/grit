#!/bin/sh
# Ported from upstream git t8001-annotate.sh

test_description='git annotate'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init annotate-repo &&
	cd annotate-repo &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&
	echo "line 1" >file &&
	echo "line 2" >>file &&
	git add file &&
	test_tick &&
	GIT_AUTHOR_NAME="Author A" git commit -m "first" &&
	echo "line 3" >>file &&
	echo "line 4" >>file &&
	git add file &&
	test_tick &&
	GIT_AUTHOR_NAME="Author B" git commit -m "second"
'

test_expect_success 'annotate runs' '
	cd annotate-repo &&
	git annotate file >actual &&
	test $(wc -l <actual) -eq 4
'

test_expect_success 'annotate shows correct authors' '
	cd annotate-repo &&
	git annotate file >actual &&
	grep "Author A" actual &&
	grep "Author B" actual
'

test_expect_success 'annotate shows line content' '
	cd annotate-repo &&
	git annotate file >actual &&
	grep "line 1" actual &&
	grep "line 4" actual
'

test_expect_success 'annotate old revision' '
	cd annotate-repo &&
	git annotate HEAD^ -- file >actual &&
	test $(wc -l <actual) -eq 2 &&
	grep "Author A" actual
'

test_expect_success 'annotate --porcelain' '
	cd annotate-repo &&
	git annotate --porcelain file >actual &&
	grep "^author " actual &&
	grep "^filename " actual
'

test_done
