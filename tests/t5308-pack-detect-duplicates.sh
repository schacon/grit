#!/bin/sh

test_description='handling of duplicate objects in pack operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "content" >file &&
	git add file &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'pack all objects' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'creating duplicate pack does not corrupt repo' '
	git pack-objects --all duppack &&
	git verify-pack duppack-*.pack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'index-pack --stdin creates valid pack' '
	git pack-objects --all --stdout >stdin.pack &&
	git index-pack --stdin <stdin.pack
'

test_done
