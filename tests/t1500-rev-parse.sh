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

test_expect_success 'inside .git directory: --is-inside-git-dir is true' '
	cd repo/.git &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git directory: --is-inside-work-tree is false' '
	cd repo/.git &&
	echo false >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git directory: --git-dir is .' '
	cd repo/.git &&
	echo . >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git/objects: --is-inside-git-dir is true' '
	cd repo/.git/objects &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git/objects: --git-dir reports parent' '
	cd repo/.git/objects &&
	grit rev-parse --git-dir >actual &&
	test "$(cat actual)" = ".." ||
	test "$(cat actual)" = "$(cd .. && pwd)"
'

test_expect_success '--show-toplevel from inside .git fails' '
	cd repo/.git &&
	test_must_fail grit rev-parse --show-toplevel
'

test_expect_success '--show-toplevel from subdirectory' '
	cd repo &&
	pwd >expect &&
	grit -C sub/dir rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success '--short=100 truncates to actual hash length' '
	cd repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&
	echo hello >commitfile &&
	grit add commitfile &&
	grit commit -m "for short test" &&
	grit rev-parse HEAD >expect &&
	grit rev-parse --short=100 HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-bare-repository is true' '
	grit init --bare bare-repo &&
	cd bare-repo &&
	echo true >expect &&
	grit rev-parse --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-inside-work-tree is false' '
	cd bare-repo &&
	echo false >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-inside-git-dir is true' '
	cd bare-repo &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --git-dir is .' '
	cd bare-repo &&
	echo . >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_done
