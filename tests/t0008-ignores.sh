#!/bin/sh
# Ported subset from git/t/t0008-ignores.sh.

test_description='grit check-ignore subset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with ignore sources' '
	grit init repo &&
	cd repo &&
	echo "ref: refs/heads/main" >.git/HEAD &&
	mkdir -p a/b/ignored-dir .git/info &&
	cat >.gitignore <<-\EOF &&
	one
	ignored-*
	top-level-dir/
	EOF
	cat >a/.gitignore <<-\EOF &&
	two*
	*three
	EOF
	cat >a/b/.gitignore <<-\EOF &&
	four
	five
	# comment to affect line numbers
	six
	ignored-dir/
	# and blank line below also counts

	!on*
	!two
	EOF
	echo per-repo >.git/info/exclude &&
	cat >global-excludes <<-\EOF &&
	globalone
	!globaltwo
	globalthree
	EOF
	: >ignored-and-untracked &&
	: >a/ignored-and-untracked &&
	: >not-ignored &&
	: >a/not-ignored &&
	: >ignored-but-in-index &&
	: >a/ignored-but-in-index &&
	grit update-index --add ignored-but-in-index a/ignored-but-in-index
'

############################################################################
#
# test invalid inputs

test_expect_success 'empty command line fails' '
	cd repo &&
	test_must_fail grit check-ignore >out 2>err &&
	grep "no path specified" err
'

test_expect_success '--stdin with extra path fails' '
	cd repo &&
	test_must_fail grit check-ignore --stdin foo >out 2>err &&
	grep "cannot specify pathnames with --stdin" err
'

test_expect_success '-z without --stdin fails' '
	cd repo &&
	test_must_fail grit check-ignore -z >out 2>err &&
	grep -- "-z only makes sense with --stdin" err
'

test_expect_success '--stdin -z with superfluous arg fails' '
	cd repo &&
	test_must_fail grit check-ignore --stdin -z foo >out 2>err &&
	grep "cannot specify pathnames with --stdin" err
'

test_expect_success '-z without --stdin and superfluous arg fails' '
	cd repo &&
	test_must_fail grit check-ignore -z foo >out 2>err &&
	grep -- "-z only makes sense with --stdin" err
'

test_expect_success '. corner-case is not ignored' '
	cd repo &&
	test_must_fail grit check-ignore . >actual 2>err &&
	test ! -s actual
'

test_expect_success '. corner-case verbose non-matching' '
	cd repo &&
	test_expect_code 1 grit check-ignore -v -n . >actual &&
	echo "::	." >expect &&
	test_cmp expect actual
'

test_expect_success '--stdin with empty STDIN exits 1' '
	cd repo &&
	test_must_fail grit check-ignore --stdin </dev/null >actual 2>err &&
	test ! -s actual
'

test_expect_success '-q with multiple args fails' '
	cd repo &&
	test_must_fail grit check-ignore -q one two >out 2>err &&
	grep "only valid with a single pathname" err
'

test_expect_success '--quiet with multiple args fails' '
	cd repo &&
	test_must_fail grit check-ignore --quiet one two >out 2>err &&
	grep "only valid with a single pathname" err
'

test_expect_success '-q -v conflict fails' '
	cd repo &&
	test_must_fail grit check-ignore -q -v foo >out 2>err &&
	grep "cannot have both --quiet and --verbose" err
'

test_expect_success '-q --verbose conflict fails' '
	cd repo &&
	test_must_fail grit check-ignore -q --verbose foo >out 2>err &&
	grep "cannot have both --quiet and --verbose" err
'

test_expect_success '--quiet -v conflict fails' '
	cd repo &&
	test_must_fail grit check-ignore --quiet -v foo >out 2>err &&
	grep "cannot have both --quiet and --verbose" err
'

test_expect_success '--quiet --verbose conflict fails' '
	cd repo &&
	test_must_fail grit check-ignore --quiet --verbose foo >out 2>err &&
	grep "cannot have both --quiet and --verbose" err
'

test_expect_success 'erroneous use of -- separator' '
	cd repo &&
	test_must_fail grit check-ignore -- >out 2>err &&
	grep "no path specified" err
'

test_expect_success 'needs work tree (run from .git dir)' '
	cd repo/.git &&
	test_must_fail grit check-ignore foo >out 2>err &&
	grep "must be run in a work tree" err
'

############################################################################
#
# test standard ignores

test_expect_success 'basic path arguments and verbose output' '
	cd repo &&
	grit check-ignore one not-ignored >actual &&
	echo one >expect &&
	test_cmp expect actual &&
	grit check-ignore -v one >actual &&
	echo ".gitignore:1:one	one" >expect &&
	test_cmp expect actual
'

test_expect_success 'tracked paths hidden unless --no-index' '
	cd repo &&
	test_must_fail grit check-ignore ignored-but-in-index >actual 2>err &&
	test ! -s actual &&
	grit check-ignore --no-index ignored-but-in-index >actual &&
	echo ignored-but-in-index >expect &&
	test_cmp expect actual
'

test_expect_success 'non-existent file not ignored' '
	cd repo &&
	test_must_fail grit check-ignore non-existent >actual 2>err &&
	test ! -s actual
'

test_expect_success 'non-existent file not ignored (verbose non-matching)' '
	cd repo &&
	test_expect_code 1 grit check-ignore -v -n non-existent >actual &&
	echo "::	non-existent" >expect &&
	test_cmp expect actual
'

test_expect_success 'non-existent file in subdir not ignored' '
	cd repo &&
	test_must_fail grit check-ignore a/non-existent >actual 2>err &&
	test ! -s actual
'

test_expect_success 'non-existent file ignored' '
	cd repo &&
	grit check-ignore one >actual &&
	echo one >expect &&
	test_cmp expect actual
'

test_expect_success 'non-existent file ignored verbose' '
	cd repo &&
	grit check-ignore -v one >actual &&
	echo ".gitignore:1:one	one" >expect &&
	test_cmp expect actual
'

test_expect_success 'non-existent file in subdir ignored' '
	cd repo &&
	grit check-ignore a/one >actual &&
	echo a/one >expect &&
	test_cmp expect actual
'

test_expect_success 'non-existent file in subdir ignored verbose' '
	cd repo &&
	grit check-ignore -v a/one >actual &&
	echo ".gitignore:1:one	a/one" >expect &&
	test_cmp expect actual
'

test_expect_success 'existing untracked file not ignored' '
	cd repo &&
	test_must_fail grit check-ignore not-ignored >actual 2>err &&
	test ! -s actual
'

test_expect_success 'existing tracked file not ignored' '
	cd repo &&
	test_must_fail grit check-ignore ignored-but-in-index >actual 2>err &&
	test ! -s actual
'

test_expect_success 'existing tracked file shown as ignored with --no-index' '
	cd repo &&
	grit check-ignore --no-index ignored-but-in-index >actual &&
	echo ignored-but-in-index >expect &&
	test_cmp expect actual
'

test_expect_success 'existing untracked file ignored' '
	cd repo &&
	grit check-ignore ignored-and-untracked >actual &&
	echo ignored-and-untracked >expect &&
	test_cmp expect actual
'

test_expect_success 'existing untracked file ignored verbose' '
	cd repo &&
	grit check-ignore -v ignored-and-untracked >actual &&
	echo ".gitignore:2:ignored-*	ignored-and-untracked" >expect &&
	test_cmp expect actual
'

test_expect_success 'mix of file types at top-level' '
	cd repo &&
	grit check-ignore -v -n \
		non-existent one not-ignored ignored-but-in-index ignored-and-untracked >actual &&
	cat >expect <<-\EOF &&
	::	non-existent
	.gitignore:1:one	one
	::	not-ignored
	::	ignored-but-in-index
	.gitignore:2:ignored-*	ignored-and-untracked
	EOF
	test_cmp expect actual
'

test_expect_success 'mix of file types in subdir a/' '
	cd repo &&
	grit check-ignore -v -n \
		a/non-existent a/one a/not-ignored a/ignored-but-in-index a/ignored-and-untracked >actual &&
	cat >expect <<-\EOF &&
	::	a/non-existent
	.gitignore:1:one	a/one
	::	a/not-ignored
	::	a/ignored-but-in-index
	.gitignore:2:ignored-*	a/ignored-and-untracked
	EOF
	test_cmp expect actual
'

############################################################################
#
# test sub-directory local ignore patterns

test_expect_success 'sub-directory local ignore' '
	cd repo &&
	grit check-ignore a/3-three a/three-not-this-one >actual &&
	echo "a/3-three" >expect &&
	test_cmp expect actual
'

test_expect_success 'sub-directory local ignore with --verbose' '
	cd repo &&
	grit check-ignore --verbose a/3-three a/three-not-this-one >actual &&
	echo "a/.gitignore:2:*three	a/3-three" >expect &&
	test_cmp expect actual
'

test_expect_success 'local ignore inside a sub-directory' '
	cd repo/a &&
	grit check-ignore 3-three three-not-this-one >actual &&
	echo "3-three" >expect &&
	test_cmp expect actual
'

test_expect_success 'local ignore inside a sub-directory with --verbose' '
	cd repo/a &&
	grit check-ignore --verbose 3-three three-not-this-one >actual &&
	echo "a/.gitignore:2:*three	3-three" >expect &&
	test_cmp expect actual
'

############################################################################
#
# test nested negation

test_expect_success 'nested include of negated pattern' '
	cd repo &&
	test_must_fail grit check-ignore a/b/one >actual 2>err &&
	test ! -s actual
'

test_expect_success 'nested include of negated pattern with -q' '
	cd repo &&
	test_must_fail grit check-ignore -q a/b/one >actual 2>err &&
	test ! -s actual
'

test_expect_success 'nested include of negated pattern with -v' '
	cd repo &&
	grit check-ignore -v a/b/one >actual &&
	echo "a/b/.gitignore:8:!on*	a/b/one" >expect &&
	test_cmp expect actual
'

test_expect_success 'nested include of negated pattern with -v -n' '
	cd repo &&
	grit check-ignore -v -n a/b/one >actual &&
	echo "a/b/.gitignore:8:!on*	a/b/one" >expect &&
	test_cmp expect actual
'

test_expect_success 'nested gitignore negation visible with verbose' '
	cd repo &&
	test_must_fail grit check-ignore a/b/one >actual 2>err &&
	test ! -s actual &&
	grit check-ignore -v a/b/one >actual &&
	echo "a/b/.gitignore:8:!on*	a/b/one" >expect &&
	test_cmp expect actual
'

############################################################################
#
# test ignored sub-directories

test_expect_success 'directory pattern applies to directory and descendants' '
	cd repo &&
	grit check-ignore a/b/ignored-dir a/b/ignored-dir/file >actual &&
	cat >expect <<-\EOF &&
	a/b/ignored-dir
	a/b/ignored-dir/file
	EOF
	test_cmp expect actual &&
	grit check-ignore -v a/b/ignored-dir/file >actual &&
	echo "a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir/file" >expect &&
	test_cmp expect actual
'

test_expect_success 'ignored sub-directory verbose' '
	cd repo &&
	grit check-ignore -v a/b/ignored-dir >actual &&
	echo "a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir" >expect &&
	test_cmp expect actual
'

test_expect_success 'multiple files inside ignored sub-directory' '
	cd repo &&
	grit check-ignore a/b/ignored-dir/foo a/b/ignored-dir/twoooo a/b/ignored-dir/seven >actual &&
	cat >expect <<-\EOF &&
	a/b/ignored-dir/foo
	a/b/ignored-dir/twoooo
	a/b/ignored-dir/seven
	EOF
	test_cmp expect actual
'

test_expect_success 'multiple files inside ignored sub-directory with -v' '
	cd repo &&
	grit check-ignore -v a/b/ignored-dir/foo a/b/ignored-dir/twoooo a/b/ignored-dir/seven >actual &&
	cat >expect <<-\EOF &&
	a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir/foo
	a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir/twoooo
	a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir/seven
	EOF
	test_cmp expect actual
'

test_expect_success 'cd to ignored sub-directory' '
	cd repo/a/b/ignored-dir &&
	grit check-ignore foo twoooo ../one seven ../../one >actual &&
	cat >expect <<-\EOF &&
	foo
	twoooo
	seven
	../../one
	EOF
	test_cmp expect actual
'

test_expect_success 'cd to ignored sub-directory with -v' '
	cd repo/a/b/ignored-dir &&
	grit check-ignore -v foo twoooo ../one seven ../../one >actual &&
	cat >expect <<-\EOF &&
	a/b/.gitignore:5:ignored-dir/	foo
	a/b/.gitignore:5:ignored-dir/	twoooo
	a/b/.gitignore:8:!on*	../one
	a/b/.gitignore:5:ignored-dir/	seven
	.gitignore:1:one	../../one
	EOF
	test_cmp expect actual
'

############################################################################
#
# test handling of global ignore files

test_expect_success 'global ignore not yet enabled' '
	cd repo &&
	grit config --unset core.excludesfile 2>/dev/null || true &&
	grit check-ignore -v globalone per-repo a/globalthree a/per-repo not-ignored a/globaltwo >actual &&
	cat >expect <<-\EOF &&
	.git/info/exclude:1:per-repo	per-repo
	a/.gitignore:2:*three	a/globalthree
	.git/info/exclude:1:per-repo	a/per-repo
	EOF
	test_cmp expect actual
'

test_expect_success 'global ignore' '
	cd repo &&
	grit config core.excludesfile global-excludes &&
	grit check-ignore globalone per-repo globalthree a/globalthree a/per-repo not-ignored globaltwo >actual &&
	cat >expect <<-\EOF &&
	globalone
	per-repo
	globalthree
	a/globalthree
	a/per-repo
	EOF
	test_cmp expect actual
'

test_expect_success 'global ignore with -v' '
	cd repo &&
	grit config core.excludesfile global-excludes &&
	grit check-ignore -v globalone per-repo globalthree a/globalthree a/per-repo not-ignored globaltwo >actual &&
	cat >expect <<-\EOF &&
	global-excludes:1:globalone	globalone
	.git/info/exclude:1:per-repo	per-repo
	global-excludes:3:globalthree	globalthree
	a/.gitignore:2:*three	a/globalthree
	.git/info/exclude:1:per-repo	a/per-repo
	global-excludes:2:!globaltwo	globaltwo
	EOF
	test_cmp expect actual
'

############################################################################
#
# test --stdin

test_expect_success '--stdin default mode' '
	cd repo &&
	grit config core.excludesfile global-excludes &&
	cat >stdin <<-\EOF &&
	one
	not-ignored
	a/b/twooo
	EOF
	grit check-ignore --stdin <stdin >actual &&
	cat >expect <<-\EOF &&
	one
	a/b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin verbose non-matching mode' '
	cd repo &&
	cat >stdin <<-\EOF &&
	one
	not-ignored
	a/b/twooo
	EOF
	grit check-ignore --stdin -v -n <stdin >actual &&
	cat >expect <<-\EOF &&
	.gitignore:1:one	one
	::	not-ignored
	a/.gitignore:1:two*	a/b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin -z emits NUL-delimited records' '
	cd repo &&
	printf "one\0not-ignored\0a/b/twooo\0" >stdin0 &&
	grit check-ignore --stdin -z <stdin0 >actual0 &&
	printf "one\0a/b/twooo\0" >expect0 &&
	test_cmp expect0 actual0 &&
	grit check-ignore --stdin -z -v <stdin0 >actual0 &&
	printf ".gitignore\0001\000one\000one\000a/.gitignore\0001\000two*\000a/b/twooo\000" >expect0 &&
	test_cmp expect0 actual0
'

test_expect_success '--stdin from subdirectory' '
	cd repo &&
	cat >stdinfile <<-\EOF &&
	../one
	../not-ignored
	one
	not-ignored
	b/on
	b/one
	b/twooo
	EOF
	(cd a && grit check-ignore --stdin <../stdinfile) >actual &&
	cat >expect <<-\EOF &&
	../one
	one
	b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin from subdirectory with -v' '
	cd repo &&
	cat >stdinfile <<-\EOF &&
	../one
	../not-ignored
	one
	not-ignored
	b/on
	b/one
	b/twooo
	EOF
	(cd a && grit check-ignore --stdin -v <../stdinfile) >actual &&
	cat >expect <<-\EOF &&
	.gitignore:1:one	../one
	.gitignore:1:one	one
	a/b/.gitignore:8:!on*	b/on
	a/b/.gitignore:8:!on*	b/one
	a/.gitignore:1:two*	b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin from subdirectory with -v -n' '
	cd repo &&
	cat >stdinfile <<-\EOF &&
	../one
	../not-ignored
	one
	not-ignored
	b/on
	b/one
	b/twooo
	EOF
	(cd a && grit check-ignore --stdin -v -n <../stdinfile) >actual &&
	cat >expect <<-\EOF &&
	.gitignore:1:one	../one
	::	../not-ignored
	.gitignore:1:one	one
	::	not-ignored
	a/b/.gitignore:8:!on*	b/on
	a/b/.gitignore:8:!on*	b/one
	a/.gitignore:1:two*	b/twooo
	EOF
	test_cmp expect actual
'

############################################################################
#
# test info/exclude and core.excludesfile precedence

test_expect_success 'info/exclude and core.excludesfile precedence' '
	cd repo &&
	grit check-ignore -v per-repo a/per-repo >actual &&
	cat >expect <<-\EOF &&
	.git/info/exclude:1:per-repo	per-repo
	.git/info/exclude:1:per-repo	a/per-repo
	EOF
	test_cmp expect actual &&
	grit config core.excludesfile global-excludes &&
	grit check-ignore -v globalone per-repo globalthree a/globalthree globaltwo >actual &&
	cat >expect <<-\EOF &&
	global-excludes:1:globalone	globalone
	.git/info/exclude:1:per-repo	per-repo
	global-excludes:3:globalthree	globalthree
	a/.gitignore:2:*three	a/globalthree
	global-excludes:2:!globaltwo	globaltwo
	EOF
	test_cmp expect actual
'

############################################################################
#
# test existing file and directory ordering

test_expect_success 'existing file and directory' '
	cd repo &&
	>one &&
	mkdir -p top-level-dir &&
	grit check-ignore one top-level-dir >actual &&
	grep one actual &&
	grep top-level-dir actual &&
	rm -f one &&
	rm -rf top-level-dir
'

test_expect_success 'existing directory and file' '
	cd repo &&
	>one &&
	mkdir -p top-level-dir &&
	grit check-ignore top-level-dir one >actual &&
	grep one actual &&
	grep top-level-dir actual &&
	rm -f one &&
	rm -rf top-level-dir
'

############################################################################
#
# test exact prefix matching

test_expect_success 'exact prefix matching (with root)' '
	cd repo &&
	rm -rf a &&
	mkdir -p a/git a/git-foo &&
	touch a/git/foo a/git-foo/bar &&
	echo /git/ >a/.gitignore &&
	grit check-ignore a/git a/git/foo a/git-foo a/git-foo/bar >actual &&
	cat >expect <<-\EOF &&
	a/git
	a/git/foo
	EOF
	test_cmp expect actual
'

test_expect_success 'exact prefix matching (without root)' '
	cd repo &&
	rm -rf a &&
	mkdir -p a/git a/git-foo &&
	touch a/git/foo a/git-foo/bar &&
	echo git/ >a/.gitignore &&
	grit check-ignore a/git a/git/foo a/git-foo a/git-foo/bar >actual &&
	cat >expect <<-\EOF &&
	a/git
	a/git/foo
	EOF
	test_cmp expect actual
'

############################################################################
#
# test ** not confused by leading prefix

test_expect_success '** not confused by matching leading prefix' '
	cd repo &&
	cat >.gitignore <<-\EOF &&
	foo**/bar
	EOF
	grit check-ignore foobar foo/bar >actual &&
	cat >expect <<-\EOF &&
	foo/bar
	EOF
	test_cmp expect actual
'

test_done
