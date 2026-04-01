#!/bin/sh
# Ported subset from git/t/t2006-checkout-index-basic.sh

test_description='grit checkout-index basic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'checkout-index --gobbledegook' '
	git init gobble &&
	(
		cd gobble &&
		test_expect_code 2 git checkout-index --gobbledegook 2>err &&
		grep -i "[Uu]sage" err
	)
'

test_expect_success 'checkout-index reports missing path (cmdline)' '
	git init repo &&
	(
		cd repo &&
		test_must_fail git checkout-index -- does-not-exist 2>stderr &&
		grep "not in" stderr
	)
'

test_expect_success 'checkout-index reports missing path (stdin)' '
	(
		cd repo &&
		echo does-not-exist |
		test_must_fail git checkout-index --stdin 2>stderr &&
		grep "not in" stderr
	)
'

test_expect_success 'checkout-index --temp correctly reports error on missing blobs' '
	git init missing-blob-repo &&
	(
		cd missing-blob-repo &&
		missing_blob=$(echo "no such blob here" | git hash-object --stdin) &&
		printf "100644 %s\tfile\n" "$missing_blob" |
			git update-index --index-info &&
		test_must_fail git checkout-index --temp file 2>stderr &&
		grep "object not found" stderr
	)
'

test_done
