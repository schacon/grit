#!/bin/sh

test_description='test index-pack handling of pack data'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "content A" >fileA &&
	echo "content B" >fileB &&
	git add fileA fileB &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'index-pack --stdin reads from stdin' '
	git pack-objects --all --stdout >test.pack &&
	git index-pack --stdin <test.pack
'

test_expect_success 'verify-pack validates indexed pack' '
	git verify-pack .git/objects/pack/pack-*.pack
'

test_expect_success 'pack-objects --revs works' '
	echo HEAD | git pack-objects --revs --stdout >revs.pack &&
	git index-pack --stdin <revs.pack
'

test_done
