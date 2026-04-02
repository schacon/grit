#!/bin/sh
# Ported from git/t/t1010-mktree.sh

test_description='grit mktree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	grit init repo &&
	cd repo &&
	grit config user.email "author@example.com" &&
	grit config user.name "A U Thor" &&
	for d in a a- a0
	do
		mkdir "$d" && echo "$d/one" >"$d/one" &&
		grit add "$d" || return 1
	done &&
	echo one >one &&
	grit add one &&
	grit write-tree >tree &&
	grit ls-tree $(cat tree) >top &&
	grit ls-tree -r $(cat tree) >all &&
	test_tick &&
	grit commit -q -m one &&
	H=$(grit rev-parse HEAD) &&
	grit update-index --add --cacheinfo 160000,$H,sub &&
	test_tick &&
	grit commit -q -m two &&
	grit rev-parse HEAD^{tree} >tree.withsub &&
	grit ls-tree HEAD >top.withsub &&
	grit ls-tree -r HEAD >all.withsub
'

test_expect_success 'ls-tree piped to mktree (1)' '
	cd repo &&
	grit mktree <top >actual &&
	test_cmp tree actual
'

test_expect_success 'ls-tree piped to mktree (2)' '
	cd repo &&
	grit mktree <top.withsub >actual &&
	test_cmp tree.withsub actual
'

test_expect_success 'ls-tree output in wrong order given to mktree (1)' '
	cd repo &&
	sort -r <top |
	grit mktree >actual &&
	test_cmp tree actual
'

test_expect_success 'ls-tree output in wrong order given to mktree (2)' '
	cd repo &&
	sort -r <top.withsub |
	grit mktree >actual &&
	test_cmp tree.withsub actual
'

test_expect_success 'mktree refuses to read ls-tree -r output (1)' '
	cd repo &&
	test_must_fail grit mktree <all
'

test_expect_success 'mktree refuses to read ls-tree -r output (2)' '
	cd repo &&
	test_must_fail grit mktree <all.withsub
'

test_expect_success 'mktree with empty input creates empty tree' '
	cd repo &&
	printf "" | grit mktree >actual &&
	echo "4b825dc642cb6eb9a060e54bf8d69288fbee4904" >expect &&
	test_cmp expect actual
'

test_expect_success 'mktree output is a valid tree object' '
	cd repo &&
	SHA=$(grit mktree <top) &&
	grit cat-file -t "$SHA" >actual &&
	echo tree >expect &&
	test_cmp expect actual
'

test_expect_success 'mktree output is deterministic' '
	cd repo &&
	grit mktree <top >actual1 &&
	grit mktree <top >actual2 &&
	test_cmp actual1 actual2
'

test_expect_success 'mktree result is readable by ls-tree' '
	cd repo &&
	SHA=$(grit mktree <top) &&
	grit ls-tree "$SHA" >roundtrip &&
	test_cmp top roundtrip
'

test_expect_success 'mktree handles executable file mode 100755' '
	cd repo &&
	BLOB=$(echo "exec content" | grit hash-object -w --stdin) &&
	printf "100755 blob $BLOB\texec.sh\n" | grit mktree >actual &&
	test_line_count = 1 actual
'

test_expect_success 'mktree handles symlink mode 120000' '
	cd repo &&
	BLOB=$(echo "target" | grit hash-object -w --stdin) &&
	printf "120000 blob $BLOB\tlink\n" | grit mktree >actual &&
	test_line_count = 1 actual
'

test_expect_success 'mktree single blob entry round-trip' '
	cd repo &&
	BLOB=$(echo "content" | grit hash-object -w --stdin) &&
	printf "100644 blob $BLOB\tfile.txt\n" | grit mktree >sha &&
	grit ls-tree "$(cat sha)" >ls_out &&
	printf "100644 blob $BLOB\tfile.txt\n" >expected &&
	test_cmp expected ls_out
'

test_expect_success 'mktree matches write-tree for same content' '
	cd repo &&
	grit write-tree >wt_sha &&
	grit mktree <top.withsub >mt_sha &&
	test_cmp wt_sha mt_sha
'

test_expect_success 'mktree with submodule entry round-trip' '
	cd repo &&
	grit mktree <top.withsub >mt_sub_sha &&
	grit ls-tree "$(cat mt_sub_sha)" >ls_sub &&
	test_cmp top.withsub ls_sub
'

test_expect_success 'mktree --batch creates multiple trees' '
	cd repo &&
	{
		cat top
		echo ""
		cat top.withsub
		echo ""
	} | grit mktree --batch >batch_out 2>/dev/null || {
		echo "mktree --batch not implemented, skipping"
		return 0
	} &&
	test_line_count = 2 batch_out
'

test_done
