#!/bin/sh
# Ported from git/t/t2107-update-index-basic.sh (harness-compatible subset).

test_description='grit update-index basic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

ZERO_OID=0000000000000000000000000000000000000000

test_expect_success 'setup repository' '
	git init repo &&
	cd repo
'

test_expect_success 'update-index --add tracks a new file' '
	cd repo &&
	echo one >one &&
	oid=$(git hash-object -w one) &&
	git update-index --add one &&
	echo "100644 $oid 0	one" >expect &&
	git ls-files --stage one >actual &&
	test_cmp expect actual
'

test_expect_success '--cacheinfo mode,oid,path adds entry' '
	cd repo &&
	echo cache >cache-src &&
	oid=$(git hash-object -w cache-src) &&
	git update-index --cacheinfo "100644,$oid,cache-only" &&
	echo "100644 $oid 0	cache-only" >expect &&
	git ls-files --stage cache-only >actual &&
	test_cmp expect actual
'

test_expect_success '--force-remove removes tracked path from index' '
	cd repo &&
	git update-index --force-remove one &&
	: >expect &&
	git ls-files one >actual &&
	test_cmp expect actual
'

test_expect_success '--index-info can add and delete an entry' '
	cd repo &&
	echo info >info-src &&
	oid=$(git hash-object -w info-src) &&
	printf "100644 %s\tfrom-index-info\n" "$oid" >stdin &&
	git update-index --index-info <stdin &&
	echo "100644 $oid 0	from-index-info" >expect &&
	git ls-files --stage from-index-info >actual &&
	test_cmp expect actual &&
	printf "0 %s\tfrom-index-info\n" "$ZERO_OID" >stdin &&
	git update-index --index-info <stdin &&
	: >expect &&
	git ls-files from-index-info >actual &&
	test_cmp expect actual
'

test_expect_success 'update-index --nonsense fails' '
	cd repo &&
	test_must_fail git update-index --nonsense 2>msg &&
	test -s msg
'

test_expect_success 'update-index --nonsense dumps usage' '
	cd repo &&
	test_expect_code 2 git update-index --nonsense 2>err &&
	grep -i "[Uu]sage" err
'

test_expect_success '--cacheinfo complains of missing arguments' '
	cd repo &&
	test_must_fail git update-index --cacheinfo 2>err &&
	test -s err
'

test_expect_success '--cacheinfo mode,sha1,path new syntax' '
	cd repo &&
	echo content >newfile &&
	sha=$(git hash-object -w --stdin <newfile) &&
	git update-index --add --cacheinfo "100644,$sha,elif" &&
	echo "100644 $sha 0	elif" >expect &&
	git ls-files --stage elif >actual &&
	test_cmp expect actual
'

test_done
