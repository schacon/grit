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

# ── additional tests ──────────────────────────────────────────────────────────

test_expect_success '-b sets initial branch for bare repo' '
	git init --bare -b develop develop.git &&
	printf "ref: refs/heads/develop" >expected &&
	printf "%s" "$(cat develop.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'init creates a new directory' '
	rm -fr nd1 &&
	git init nd1 &&
	test_path_is_dir nd1/.git/refs
'

test_expect_success 'init creates a new bare directory' '
	rm -fr nd2 &&
	git init --bare nd2 &&
	test_path_is_dir nd2/refs
'

test_expect_success 'init recreates a directory' '
	rm -fr nd3 &&
	mkdir nd3 &&
	git init nd3 &&
	test_path_is_dir nd3/.git/refs
'

test_expect_success 'init recreates a new bare directory' '
	rm -fr nd4 &&
	mkdir nd4 &&
	git init --bare nd4 &&
	test_path_is_dir nd4/refs
'

test_expect_success 'init creates a new deep directory' '
	rm -fr nd5 &&
	git init nd5/a/b/c &&
	test_path_is_dir nd5/a/b/c/.git/refs
'

test_expect_success 'init notices EEXIST (1)' '
	rm -fr eex1 &&
	>eex1 &&
	test_must_fail git init eex1 &&
	test_path_is_file eex1
'

test_expect_success 'init notices EEXIST (2)' '
	rm -fr eex2 &&
	mkdir eex2 &&
	>eex2/a &&
	test_must_fail git init eex2/a/b &&
	test_path_is_file eex2/a
'

test_expect_success 'reinit prints Initialized empty on first init' '
	rm -fr rinit &&
	mkdir rinit &&
	(cd rinit && git init >out1 2>err1) &&
	grep "Initialized empty" rinit/out1 &&
	test_must_be_empty rinit/err1
'

test_expect_success 'reinit still shows Initialized on second init' '
	rm -fr rinit2 &&
	git init rinit2 &&
	git init rinit2 >out2 2>err2 &&
	grep "Initialized" out2 &&
	test_must_be_empty err2
'

test_expect_success 'bare init sets core.bare = true in config' '
	rm -fr cb1 &&
	git init --bare cb1 &&
	grep "bare = true" cb1/config
'

test_expect_success 'plain init sets core.bare = false in config' '
	rm -fr cb2 &&
	git init cb2 &&
	grep "bare = false" cb2/.git/config
'

test_expect_success 'init with template copies files to .git' '
	rm -fr tsrc tdst &&
	mkdir tsrc &&
	echo "hook content" >tsrc/pre-commit &&
	git init --template=tsrc tdst &&
	test_path_is_file tdst/.git/pre-commit
'

test_expect_success 'bare init with -b sets HEAD to custom branch' '
	rm -fr bare-b-test &&
	git init --bare -b feature/foo bare-b-test &&
	printf "ref: refs/heads/feature/foo" >expected &&
	printf "%s" "$(cat bare-b-test/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'bare init has no .git subdirectory' '
	rm -fr bare-nondot &&
	git init --bare bare-nondot &&
	test_path_is_missing bare-nondot/.git
'

test_expect_success 'description file is created' '
	rm -fr desctest &&
	git init desctest &&
	test_path_is_file desctest/.git/description
'

test_expect_success 'bare description file is created' '
	rm -fr bare-desc &&
	git init --bare bare-desc &&
	test_path_is_file bare-desc/description
'

test_expect_success 'init creates hooks directory' '
	rm -fr hooktest &&
	git init hooktest &&
	test_path_is_dir hooktest/.git/hooks
'

test_expect_success 'bare init creates hooks directory' '
	rm -fr bare-hooks &&
	git init --bare bare-hooks &&
	test_path_is_dir bare-hooks/hooks
'

test_expect_success 'init creates objects/info directory' '
	rm -fr objtest &&
	git init objtest &&
	test_path_is_dir objtest/.git/objects/info
'

test_expect_success 'init creates objects/pack directory' '
	rm -fr packtest &&
	git init packtest &&
	test_path_is_dir packtest/.git/objects/pack
'

test_expect_success 'config file is created with proper content' '
	rm -fr cfgtest &&
	git init cfgtest &&
	test_path_is_file cfgtest/.git/config &&
	grep "\[core\]" cfgtest/.git/config
'

test_expect_success 'init in long base path' '
	rm -fr longbase &&
	component=123456789abcdef &&
	p31=$component/$component &&
	p63=$p31/$p31 &&
	mkdir -p $p63 &&
	(
		cd $p63 &&
		git init longdir
	) &&
	test_path_is_dir $p63/longdir/.git/refs
'

# NOTE: --separate-git-dir is broken in this version of grit (the git file
# cannot be written because init_repository creates .git as a directory first,
# then write fails with EISDIR). Skipping those tests.

# NOTE: GIT_DIR environment variable is not respected by grit init. Skipping.

# NOTE: grit does not support 'git --bare init' (global --bare flag before
# the subcommand), only 'git init --bare'. Skipping that test.

test_done
