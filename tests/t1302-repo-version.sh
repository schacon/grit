#!/bin/sh
# Test repository format version handling and extensions.

test_description='grit repository format version and extensions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Default repository format version
###########################################################################

test_expect_success 'init creates repo with version 0' '
	grit init default-repo &&
	cd default-repo &&
	grit config core.repositoryformatversion >actual &&
	echo "0" >expect &&
	test_cmp expect actual
'

test_expect_success 'init creates standard config keys' '
	cd default-repo &&
	grit config core.filemode >actual &&
	test "$(cat actual)" = "true" &&
	grit config core.bare >actual &&
	test "$(cat actual)" = "false"
'

test_expect_success 'init sets logallrefupdates for non-bare' '
	cd default-repo &&
	grit config core.logallrefupdates >actual &&
	test "$(cat actual)" = "true"
'

test_expect_success 'bare repo has bare=true' '
	grit init --bare bare-repo.git &&
	grit config -f bare-repo.git/config core.bare >actual &&
	test "$(cat actual)" = "true"
'

test_expect_success 'bare repo has version 0' '
	grit config -f bare-repo.git/config core.repositoryformatversion >actual &&
	echo "0" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 2: Setting repo version manually
###########################################################################

test_expect_success 'setup: create working repo' '
	grit init work-repo &&
	cd work-repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "content" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial"
'

test_expect_success 'can set repositoryformatversion to 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	grit config core.repositoryformatversion >actual &&
	echo "1" >expect &&
	test_cmp expect actual
'

test_expect_success 'basic operations work with version 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	grit status >actual 2>&1 &&
	grit rev-parse HEAD >actual
'

test_expect_success 'can read objects with version 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	OID=$(grit rev-parse HEAD) &&
	grit cat-file -t "$OID" >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'add and commit work with version 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	echo "more" >file2.txt &&
	grit add file2.txt &&
	grit commit -m "second commit"
'

test_expect_success 'ls-files works with version 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	grit ls-files >actual &&
	grep "file.txt" actual &&
	grep "file2.txt" actual
'

test_expect_success 'log works with version 1' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	grit log >actual &&
	grep "initial" actual &&
	grep "second commit" actual
'

###########################################################################
# Section 3: High version numbers
###########################################################################

test_expect_success 'status works with high version number' '
	cd work-repo &&
	grit config core.repositoryformatversion 99 &&
	grit status >actual 2>&1
'

test_expect_success 'rev-parse works with high version number' '
	cd work-repo &&
	grit config core.repositoryformatversion 99 &&
	grit rev-parse HEAD >actual
'

test_expect_success 'reset version to 0' '
	cd work-repo &&
	grit config core.repositoryformatversion 0 &&
	grit config core.repositoryformatversion >actual &&
	echo "0" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 4: Extensions
###########################################################################

test_expect_success 'setting an extension via config' '
	cd work-repo &&
	grit config core.repositoryformatversion 1 &&
	grit config extensions.noop true &&
	grit config extensions.noop >actual &&
	echo "true" >expect &&
	test_cmp expect actual
'

test_expect_success 'status works with unknown extension' '
	cd work-repo &&
	grit config extensions.noop true &&
	grit status >actual 2>&1
'

test_expect_success 'ls-files works with unknown extension' '
	cd work-repo &&
	grit config extensions.noop true &&
	grit ls-files >actual &&
	grep "file.txt" actual
'

test_expect_success 'config --list shows extensions' '
	cd work-repo &&
	grit config extensions.noop true &&
	grit config --list >actual &&
	grep "extensions.noop=true" actual
'

test_expect_success 'can set objectformat extension' '
	cd work-repo &&
	grit config extensions.objectformat sha256 &&
	grit config extensions.objectformat >actual &&
	echo "sha256" >expect &&
	test_cmp expect actual
'

test_expect_success 'can unset extension' '
	cd work-repo &&
	grit config --unset extensions.objectformat &&
	test_must_fail grit config extensions.objectformat
'

###########################################################################
# Section 5: Config file manipulation
###########################################################################

test_expect_success 'manually edited config is readable' '
	grit init manual-repo &&
	cd manual-repo &&
	cat >.git/config <<-\EOF &&
	[core]
		repositoryformatversion = 1
		filemode = true
		bare = false
	[extensions]
		noop = false
	EOF
	grit config core.repositoryformatversion >actual &&
	echo "1" >expect &&
	test_cmp expect actual
'

test_expect_success 'config with missing version key' '
	grit init no-version-repo &&
	cd no-version-repo &&
	cat >.git/config <<-\EOF &&
	[core]
		filemode = true
		bare = false
	EOF
	grit status >actual 2>&1
'

test_expect_success 'config with version 0 and no extensions' '
	grit init v0-repo &&
	cd v0-repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "data" >f.txt &&
	grit add f.txt &&
	grit commit -m "v0 commit" &&
	grit config core.repositoryformatversion >actual &&
	echo "0" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 6: Bare repo operations
###########################################################################

test_expect_success 'bare repo version preserved across config changes' '
	grit init --bare preserved.git &&
	grit config -f preserved.git/config core.repositoryformatversion >actual &&
	echo "0" >expect &&
	test_cmp expect actual &&
	grit config -f preserved.git/config some.key somevalue &&
	grit config -f preserved.git/config core.repositoryformatversion >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repo config --list works' '
	grit config -f preserved.git/config --list >actual &&
	grep "core.repositoryformatversion=0" actual &&
	grep "core.bare=true" actual
'

test_expect_success 'get-all on repo version' '
	cd work-repo &&
	grit config --get-all core.repositoryformatversion >actual &&
	test_line_count = 1 actual
'

test_done
