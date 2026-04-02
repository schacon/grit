#!/bin/sh
# Tests for .mailmap support in grit.
# Upstream git t4203 tests mailmap-based author/committer rewriting.
# grit currently supports %an/%ae but not %aN/%aE (mailmap variants),
# so we test the format codes that exist and verify .mailmap file handling.

test_description='mailmap and log author formatting'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup mailmap test repo' '
	git init mailmap-repo &&
	cd mailmap-repo &&
	git config user.name "Original Author" &&
	git config user.email "original@example.com" &&
	echo "first content" >file.txt &&
	git add file.txt &&
	test_tick &&
	git commit -m "first commit" &&
	echo "second content" >>file.txt &&
	git add file.txt &&
	test_tick &&
	git commit -m "second commit"
'

test_expect_success 'log --format=%an shows author name' '
	cd mailmap-repo &&
	git log --format="%an" --max-count 1 >actual &&
	echo "Original Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%ae shows author email' '
	cd mailmap-repo &&
	git log --format="%ae" --max-count 1 >actual &&
	echo "original@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%cn shows committer name' '
	cd mailmap-repo &&
	git log --format="%cn" --max-count 1 >actual &&
	echo "Original Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%ce shows committer email' '
	cd mailmap-repo &&
	git log --format="%ce" --max-count 1 >actual &&
	echo "original@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'log shows all commits with format' '
	cd mailmap-repo &&
	git log --format="%s" >actual &&
	cat >expect <<-\EOF &&
	second commit
	first commit
	EOF
	test_cmp expect actual
'

test_expect_success 'log --format=%H gives full sha' '
	cd mailmap-repo &&
	git log --format="%H" --max-count 1 >actual &&
	test $(wc -c <actual) -ge 40
'

test_expect_success 'log --format=%h gives abbreviated sha' '
	cd mailmap-repo &&
	git log --format="%h" --max-count 1 >actual &&
	len=$(wc -c <actual | tr -d " ") &&
	test "$len" -ge 4 &&
	test "$len" -le 20
'

test_expect_success 'setup repo with multiple authors' '
	cd mailmap-repo &&
	git config user.name "Second Author" &&
	git config user.email "second@example.com" &&
	echo "third content" >>file.txt &&
	git add file.txt &&
	test_tick &&
	git commit -m "third commit"
'

test_expect_success 'log shows different authors correctly' '
	cd mailmap-repo &&
	git log --format="%an" >actual &&
	grep "Second Author" actual &&
	grep "Original Author" actual
'

test_expect_success 'log --format with combined fields' '
	cd mailmap-repo &&
	git log --format="%an <%ae>" --max-count 1 >actual &&
	echo "Second Author <second@example.com>" >expect &&
	test_cmp expect actual
'

test_expect_success 'create .mailmap file' '
	cd mailmap-repo &&
	echo "Proper Name <proper@email.com> Original Author <original@example.com>" >.mailmap &&
	git add .mailmap &&
	test_tick &&
	git commit -m "add mailmap"
'

test_expect_success 'log %an still shows original name (no mailmap rewrite in grit)' '
	cd mailmap-repo &&
	git log --format="%an" >actual &&
	grep "Original Author" actual
'

test_expect_success '.mailmap file exists and is tracked' '
	cd mailmap-repo &&
	git ls-files --cached .mailmap >actual &&
	grep ".mailmap" actual
'

test_expect_success 'setup: complex .mailmap with multiple entries' '
	cd mailmap-repo &&
	cat >.mailmap <<-\EOF &&
	Proper Name <proper@email.com> Original Author <original@example.com>
	Real Second <real@example.com> Second Author <second@example.com>
	EOF
	git add .mailmap &&
	test_tick &&
	git commit -m "update mailmap"
'

test_expect_success '.mailmap file is in ls-files output' '
	cd mailmap-repo &&
	git ls-files >actual &&
	grep ".mailmap" actual
'

test_expect_success '.mailmap file content is preserved' '
	cd mailmap-repo &&
	test_path_is_file .mailmap &&
	grep "Proper Name" .mailmap &&
	grep "Real Second" .mailmap
'

test_expect_success 'log --format=%P shows parent hash' '
	cd mailmap-repo &&
	git log --format="%P" --max-count 1 >actual &&
	test -s actual
'

test_expect_success 'log --format=%p shows abbreviated parent' '
	cd mailmap-repo &&
	git log --format="%p" --max-count 1 >actual &&
	test -s actual
'

test_expect_success 'log --format=%T shows tree hash' '
	cd mailmap-repo &&
	git log --format="%T" --max-count 1 >actual &&
	test $(wc -c <actual) -ge 40
'

test_expect_success 'log --format=%t shows abbreviated tree' '
	cd mailmap-repo &&
	git log --format="%t" --max-count 1 >actual &&
	len=$(wc -c <actual | tr -d " ") &&
	test "$len" -ge 4
'

test_expect_success 'log --oneline output is single line per commit' '
	cd mailmap-repo &&
	git log --oneline >actual &&
	test_line_count = 5 actual
'

test_expect_success 'log --max-count limits output' '
	cd mailmap-repo &&
	git log --oneline --max-count 2 >actual &&
	test_line_count = 2 actual
'

test_expect_success 'log --max-count=1 shows only HEAD' '
	cd mailmap-repo &&
	git log --format="%s" --max-count 1 >actual &&
	echo "update mailmap" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%s shows subject' '
	cd mailmap-repo &&
	git log --format="%s" --max-count 1 >actual &&
	test -s actual &&
	! grep -q "^$" actual
'

test_expect_success 'cat-file shows .mailmap blob content' '
	cd mailmap-repo &&
	tree=$(git rev-parse HEAD^{tree}) &&
	blob=$(git ls-tree $tree | grep ".mailmap" | cut -f1 | awk "{print \$3}") &&
	git cat-file -p $blob >actual &&
	grep "Proper Name" actual
'

test_expect_success 'show commit with .mailmap change' '
	cd mailmap-repo &&
	git show HEAD >actual 2>&1 &&
	grep "update mailmap" actual
'

test_done
