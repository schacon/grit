#!/bin/sh
# Test shared repository configuration (core.sharedRepository)

test_description='grit shared repository (core.sharedRepository) config'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'init --shared is not yet supported' '
	test_must_fail grit init --shared=group shared-repo 2>stderr &&
	# Should error on unknown flag
	test -s stderr
'

test_expect_success 'setup repo and set core.sharedRepository via config' '
	grit init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	grit config core.sharedRepository group
'

test_expect_success 'core.sharedRepository can be read back' '
	cd repo &&
	grit config core.sharedRepository >actual &&
	echo "group" >expect &&
	test_cmp expect actual
'

test_expect_success 'core.sharedRepository is stored in .git/config' '
	cd repo &&
	grep -i "sharedrepository" .git/config &&
	grep "group" .git/config
'

test_expect_success 'set core.sharedRepository to 0664' '
	cd repo &&
	grit config core.sharedRepository 0664 &&
	grit config core.sharedRepository >actual &&
	echo "0664" >expect &&
	test_cmp expect actual
'

test_expect_success 'set core.sharedRepository to all' '
	cd repo &&
	grit config core.sharedRepository all &&
	grit config core.sharedRepository >actual &&
	echo "all" >expect &&
	test_cmp expect actual
'

test_expect_success 'set core.sharedRepository to true' '
	cd repo &&
	grit config core.sharedRepository true &&
	grit config core.sharedRepository >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'unset core.sharedRepository' '
	cd repo &&
	grit config --unset core.sharedRepository &&
	test_must_fail grit config core.sharedRepository 2>/dev/null
'

test_expect_success 'commits work with sharedRepository set' '
	cd repo &&
	grit config core.sharedRepository group &&
	echo "content" >file.txt &&
	grit add file.txt &&
	grit commit -m "shared repo commit" &&
	grit log --format="%s" -n1 >actual &&
	echo "shared repo commit" >expect &&
	test_cmp expect actual
'

test_done
