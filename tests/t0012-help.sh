#!/bin/sh
# Tests for grit help/usage output (--help, -h, help subcommand).

test_description='grit help and usage output'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'grit --help shows usage with command list' '
	cd repo &&
	git --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "Commands:" out
'

test_expect_success 'grit -h shows same output as --help' '
	cd repo &&
	git --help >expect 2>&1 &&
	git -h >actual 2>&1 &&
	test_cmp expect actual
'

test_expect_success 'grit help shows same output as --help' '
	cd repo &&
	git --help >expect 2>&1 &&
	git help >actual 2>&1 &&
	test_cmp expect actual
'

test_expect_success 'grit --version prints version string' '
	cd repo &&
	git --version >out 2>&1 &&
	grep "grit" out
'

test_expect_success 'grit -V prints version string' '
	cd repo &&
	git -V >out 2>&1 &&
	grep "grit" out
'

test_expect_success 'help lists add command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "add" out
'

test_expect_success 'help lists branch command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "branch" out
'

test_expect_success 'help lists commit command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "commit" out
'

test_expect_success 'help lists diff command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "diff" out
'

test_expect_success 'help lists log command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "log" out
'

test_expect_success 'help lists status command' '
	cd repo &&
	git --help >out 2>&1 &&
	grep "status" out
'

test_expect_success 'grit branch --help shows branch usage' '
	cd repo &&
	git branch --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "branch" out
'

test_expect_success 'grit commit --help shows commit usage' '
	cd repo &&
	git commit --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "commit" out
'

test_expect_success 'grit diff --help shows diff usage' '
	cd repo &&
	git diff --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "diff" out
'

test_expect_success 'grit log --help shows log usage' '
	cd repo &&
	git log --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "log" out
'

test_expect_success 'grit status --help shows status usage' '
	cd repo &&
	git status --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "status" out
'

test_expect_success 'grit init --help shows init usage' '
	cd repo &&
	git init --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "init" out
'

test_expect_success 'unknown subcommand fails with error' '
	cd repo &&
	test_must_fail git nonsense 2>err &&
	grep -i "unrecognized subcommand" err
'

test_expect_success 'unknown subcommand suggests --help' '
	cd repo &&
	test_must_fail git nonsense 2>err &&
	grep -i "\-\-help" err
'

test_expect_success 'grit with no arguments shows usage or error' '
	cd repo &&
	git >out 2>&1 || true &&
	test -s out
'

test_expect_success 'help for each-ref shows for-each-ref usage' '
	cd repo &&
	git for-each-ref --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "for-each-ref" out
'

test_expect_success 'help for ls-remote shows ls-remote usage' '
	cd repo &&
	git ls-remote --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "ls-remote" out
'

test_expect_success 'help for rev-parse shows rev-parse usage' '
	cd repo &&
	git rev-parse --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "rev-parse" out
'

test_expect_success 'help for tag shows tag usage' '
	cd repo &&
	git tag --help >out 2>&1 &&
	grep -i "usage" out &&
	grep "tag" out
'

test_done
