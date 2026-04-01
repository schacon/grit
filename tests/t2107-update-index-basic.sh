#!/bin/sh
# Ported from git/t/t2107-update-index-basic.sh (harness-compatible subset).

test_description='gust update-index basic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

ZERO_OID=0000000000000000000000000000000000000000

test_expect_success 'setup repository' '
	gust init repo &&
	cd repo
'

test_expect_success 'update-index --add tracks a new file' '
	cd repo &&
	echo one >one &&
	oid=$(gust hash-object -w one) &&
	gust update-index --add one &&
	echo "100644 $oid 0	one" >expect &&
	gust ls-files --stage one >actual &&
	test_cmp expect actual
'

test_expect_success '--cacheinfo mode,oid,path adds entry' '
	cd repo &&
	echo cache >cache-src &&
	oid=$(gust hash-object -w cache-src) &&
	gust update-index --cacheinfo "100644,$oid,cache-only" &&
	echo "100644 $oid 0	cache-only" >expect &&
	gust ls-files --stage cache-only >actual &&
	test_cmp expect actual
'

test_expect_success '--force-remove removes tracked path from index' '
	cd repo &&
	gust update-index --force-remove one &&
	: >expect &&
	gust ls-files one >actual &&
	test_cmp expect actual
'

test_expect_success '--index-info can add and delete an entry' '
	cd repo &&
	echo info >info-src &&
	oid=$(gust hash-object -w info-src) &&
	printf "100644 %s\tfrom-index-info\n" "$oid" >stdin &&
	gust update-index --index-info <stdin &&
	echo "100644 $oid 0	from-index-info" >expect &&
	gust ls-files --stage from-index-info >actual &&
	test_cmp expect actual &&
	printf "0 %s\tfrom-index-info\n" "$ZERO_OID" >stdin &&
	gust update-index --index-info <stdin &&
	: >expect &&
	gust ls-files from-index-info >actual &&
	test_cmp expect actual
'

test_done
