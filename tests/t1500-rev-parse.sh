#!/bin/sh
# Ported subset from git/t/t1500-rev-parse.sh.

test_description='grit rev-parse discovery flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with nested directory' '
	grit init repo &&
	cd repo &&
	echo hello >hello.txt &&
	grit hash-object -w hello.txt >/dev/null &&
	mkdir -p sub/dir
'

test_expect_success '--is-inside-work-tree true in repository root' '
	cd repo &&
	echo true >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--is-inside-work-tree true in subdirectory' '
	cd repo/sub/dir &&
	echo true >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix reports relative subdirectory path' '
	cd repo/sub/dir &&
	echo sub/dir/ >expect &&
	grit rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix is empty at work-tree root' '
	cd repo &&
	echo >expect &&
	grit rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--git-dir returns relative path from root and subdirectory' '
	cd repo &&
	echo .git >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual &&
	cd sub/dir &&
	echo ../../.git >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success '--show-toplevel returns repository root' '
	cd repo/sub/dir &&
	pwd_root=$(cd ../.. && pwd) &&
	echo "$pwd_root" >expect &&
	grit rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success 'outside repository prints false for --is-inside-work-tree' '
	cd .. &&
	echo false >expect &&
	GIT_DIR=does-not-exist grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--is-bare-repository false in non-bare repository' '
	cd repo &&
	echo false >expect &&
	grit rev-parse --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success '--is-inside-git-dir false in work tree' '
	cd repo &&
	echo false >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'outside repository: --is-inside-git-dir prints false' '
	cd .. &&
	echo false >expect &&
	GIT_DIR=does-not-exist grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple discovery flags in one invocation' '
	cd repo &&
	printf "true\nfalse\n" >expect &&
	grit rev-parse --is-inside-work-tree --is-bare-repository >actual &&
	test_cmp expect actual
'

test_done
