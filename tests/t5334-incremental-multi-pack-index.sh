#!/bin/sh

test_description='incremental multi-pack-index'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit base
'

test_expect_failure 'write midx' '
	git multi-pack-index write
'

test_expect_failure 'write incremental midx' '
	git multi-pack-index write --incremental
'

test_done
