#!/bin/sh
# Ported subset from git/t/t2006-checkout-index-basic.sh

test_description='grit checkout-index basic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'checkout-index reports missing path (cmdline)' '
	grit init repo &&
	cd repo &&
	test_must_fail grit checkout-index -- does-not-exist 2>stderr &&
	grep "not in index" stderr
'

test_expect_success 'checkout-index reports missing path (stdin)' '
	cd repo &&
	echo does-not-exist |
	test_must_fail grit checkout-index --stdin 2>stderr &&
	grep "not in index" stderr
'

test_done
