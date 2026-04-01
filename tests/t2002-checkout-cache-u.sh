#!/bin/sh
# Ported subset from git/t/t2002-checkout-cache-u.sh

test_description='grit checkout-index -u'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup index and tree' '
	grit init repo &&
	cd repo &&
	echo frotz >path0 &&
	grit update-index --add path0 &&
	grit write-tree >tree_oid
'

test_expect_success 'without -u checkout-index does not rewrite index stat data' '
	cd repo &&
	rm -f path0 &&
	t=$(cat tree_oid) &&
	grit read-tree "$t" &&
	before=$(cksum .git/index) &&
	grit checkout-index -f -a &&
	after=$(cksum .git/index) &&
	test "x$before" = "x$after"
'

test_expect_success 'with -u checkout-index rewrites index stat data' '
	cd repo &&
	rm -f path0 &&
	t=$(cat tree_oid) &&
	grit read-tree "$t" &&
	before=$(cksum .git/index) &&
	grit checkout-index -u -f -a &&
	after=$(cksum .git/index) &&
	test "x$before" != "x$after"
'

test_done
