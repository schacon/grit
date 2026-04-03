#!/bin/sh

test_description='compact multi-pack-index'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit base
'

test_expect_failure 'write midx' '
	git multi-pack-index write
'

test_expect_failure 'compact midx' '
	git multi-pack-index write --incremental &&
	git multi-pack-index write --incremental &&
	git multi-pack-index compact
'

test_done
