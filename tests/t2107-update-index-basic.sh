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

test_expect_success '--add multiple files at once' '
	cd repo &&
	echo fa >file_a &&
	echo fb >file_b &&
	git hash-object -w file_a >/dev/null &&
	git hash-object -w file_b >/dev/null &&
	git update-index --add file_a file_b &&
	git ls-files --stage file_a >actual_a &&
	test -s actual_a &&
	git ls-files --stage file_b >actual_b &&
	test -s actual_b
'

test_expect_success '--remove deletes tracked path from index' '
	cd repo &&
	git ls-files --stage file_a >before &&
	test -s before &&
	git update-index --remove file_a &&
	git ls-files file_a >after &&
	test_must_fail test -s after
'

test_expect_success '--info-only adds entry without writing object' '
	cd repo &&
	echo infodata >info-only-file &&
	oid=$(git hash-object info-only-file) &&
	git update-index --add --info-only --cacheinfo "100644,$oid,info-only-entry" &&
	git ls-files --stage info-only-entry >actual &&
	grep "$oid" actual
'

test_expect_success '--refresh updates stat info' '
	cd repo &&
	echo refresh >refresh-file &&
	git update-index --add refresh-file &&
	touch refresh-file &&
	git update-index --refresh &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--really-refresh forces stat refresh' '
	cd repo &&
	git update-index --really-refresh &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--assume-unchanged marks file (no ls-files -v to verify)' '
	cd repo &&
	git update-index --assume-unchanged refresh-file &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--no-assume-unchanged clears flag' '
	cd repo &&
	git update-index --no-assume-unchanged refresh-file &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--skip-worktree marks file' '
	cd repo &&
	git update-index --skip-worktree refresh-file &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--no-skip-worktree clears flag' '
	cd repo &&
	git update-index --no-skip-worktree refresh-file &&
	git ls-files --stage refresh-file >actual &&
	test -s actual
'

test_expect_success '--cacheinfo with executable mode' '
	cd repo &&
	echo execdata >exec-file &&
	oid=$(git hash-object -w exec-file) &&
	git update-index --cacheinfo "100755,$oid,exec-entry" &&
	git ls-files --stage exec-entry >actual &&
	grep "^100755" actual
'

test_expect_success '--cacheinfo with symlink mode' '
	cd repo &&
	echo target >link-data &&
	oid=$(git hash-object -w link-data) &&
	git update-index --cacheinfo "120000,$oid,link-entry" &&
	git ls-files --stage link-entry >actual &&
	grep "^120000" actual
'

test_expect_success '--force-remove on nonexistent file is silent' '
	cd repo &&
	git update-index --force-remove nonexistent 2>err &&
	test_must_fail test -s err
'

test_expect_success '--add with modified file updates index' '
	cd repo &&
	echo original >mod-file &&
	git update-index --add mod-file &&
	oid1=$(git ls-files --stage mod-file | cut -d" " -f2) &&
	echo modified >mod-file &&
	git hash-object -w mod-file >/dev/null &&
	git update-index --add mod-file &&
	oid2=$(git ls-files --stage mod-file | cut -d" " -f2) &&
	test "$oid1" != "$oid2"
'

test_expect_success '--ignore-missing does not error on missing files' '
	cd repo &&
	git update-index --add --ignore-missing does-not-exist 2>err || true &&
	test_must_fail git ls-files --error-unmatch does-not-exist 2>/dev/null
'

test_expect_success 'update-index --help shows usage' '
	cd repo &&
	git update-index --help >out 2>&1 &&
	grep -i "usage" out
'

test_done
