#!/bin/sh
# Ported subset from git/t/t1500-rev-parse.sh.

test_description='gust rev-parse discovery flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with nested directory' '
	gust init repo &&
	cd repo &&
	echo hello >hello.txt &&
	gust hash-object -w hello.txt >/dev/null &&
	mkdir -p sub/dir
'

test_expect_success '--is-inside-work-tree true in repository root' '
	cd repo &&
	echo true >expect &&
	gust rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--is-inside-work-tree true in subdirectory' '
	cd repo/sub/dir &&
	echo true >expect &&
	gust rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix reports relative subdirectory path' '
	cd repo/sub/dir &&
	echo sub/dir/ >expect &&
	gust rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix is empty at work-tree root' '
	cd repo &&
	echo >expect &&
	gust rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--git-dir returns relative path from root and subdirectory' '
	cd repo &&
	echo .git >expect &&
	gust rev-parse --git-dir >actual &&
	test_cmp expect actual &&
	cd sub/dir &&
	echo ../../.git >expect &&
	gust rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success '--show-toplevel returns repository root' '
	cd repo/sub/dir &&
	pwd_root=$(cd ../.. && pwd) &&
	echo "$pwd_root" >expect &&
	gust rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success 'outside repository prints false for --is-inside-work-tree' '
	cd .. &&
	echo false >expect &&
	GIT_DIR=does-not-exist gust rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_done
