#!/bin/sh

test_description='git pack-object with large objects'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	printf "%100000s" X | git hash-object -w --stdin >oid &&
	git pack-objects testpack <oid &&
	git verify-pack testpack-*.pack
'

test_expect_success 'large object roundtrip via pack' '
	oid=$(cat oid) &&
	git cat-file blob $oid >actual &&
	printf "%100000s" X >expect &&
	test_cmp expect actual
'

test_done
