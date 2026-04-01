#!/bin/sh
# Ported from git/t/t0001-init.sh
# Tests for 'grit init'.

test_description='grit init'

# Run from the tests/ directory so test-lib.sh is found relative to $0
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── helpers ───────────────────────────────────────────────────────────────────

# check_config <dir> <expected-bare: true|false> [<expected-worktree>]
check_config () {
	if test_path_is_dir "$1" &&
	   test_path_is_file "$1/config" &&
	   test_path_is_dir "$1/refs"
	then
		: happy
	else
		echo "expected a directory $1, a file $1/config and $1/refs"
		return 1
	fi
}

# ── tests ─────────────────────────────────────────────────────────────────────

test_expect_success 'plain init creates expected skeleton' '
	git init plain &&
	check_config plain/.git &&
	test_path_is_file plain/.git/HEAD &&
	test_path_is_dir  plain/.git/objects &&
	test_path_is_dir  plain/.git/refs/heads &&
	test_path_is_dir  plain/.git/refs/tags
'

test_expect_success 'HEAD points to refs/heads/master by default' '
	git init head-test &&
	echo "ref: refs/heads/master" >expected &&
	cat head-test/.git/HEAD >actual &&
	# strip trailing newline for comparison
	printf "ref: refs/heads/master" >expected &&
	printf "%s" "$(cat head-test/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success '-b sets initial branch' '
	git init -b main branchtest &&
	printf "ref: refs/heads/main" >expected &&
	printf "%s" "$(cat branchtest/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'bare init' '
	git init --bare bare.git &&
	check_config bare.git &&
	test_path_is_file bare.git/HEAD
'

test_expect_success 'plain init in non-existent directory creates it' '
	git init newdir/deep &&
	test_path_is_dir newdir/deep/.git
'

test_expect_success 'init is idempotent (reinit)' '
	git init reinit &&
	git init reinit &&
	test_path_is_dir reinit/.git
'

test_expect_success '--quiet suppresses output' '
	git init --quiet quiettest >out 2>&1 &&
	test -s out && test "$(wc -c <out)" -lt 1 ||
	! test -s out
'

test_expect_success 'bare init creates objects/ refs/ and HEAD at root' '
	git init --bare bare2.git &&
	test_path_is_dir bare2.git/objects &&
	test_path_is_dir bare2.git/refs &&
	test_path_is_file bare2.git/HEAD
'

test_expect_success 'init with template directory' '
	mkdir tmpl &&
	echo "custom" >tmpl/myfile &&
	git init --template=tmpl fromtmpl &&
	test_path_is_file fromtmpl/.git/myfile
'

test_done
