#!/bin/sh
#
# Tests for rev-list --filter (blob:none, tree:0, blob:limit, combine, etc.)
# and --objects

test_description='rev-list object filtering (--filter, --objects)'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

test_expect_success 'setup repository with various objects' '
	git init -b main . &&

	echo "small content" >small.txt &&
	git add small.txt &&
	test_tick &&
	git commit -m "add small file" &&
	git tag first &&

	mkdir subdir &&
	echo "nested content" >subdir/nested.txt &&
	git add subdir &&
	test_tick &&
	git commit -m "add nested file" &&
	git tag second &&

	# Create a larger file
	dd if=/dev/zero bs=1024 count=10 2>/dev/null | tr "\0" "x" >large.bin &&
	git add large.bin &&
	test_tick &&
	git commit -m "add large binary" &&
	git tag third &&

	echo "another" >another.txt &&
	git add another.txt &&
	test_tick &&
	git commit -m "add another" &&
	git tag fourth
'


test_expect_success 'rev-list --objects lists commits and objects' '
	git rev-list --objects HEAD >actual &&
	# Should include commit SHAs plus tree/blob SHAs with paths
	test $(wc -l <actual) -gt 4
'

test_expect_success 'rev-list --objects HEAD lists blobs with paths' '
	git rev-list --objects HEAD >actual &&
	grep "small.txt" actual
'

test_expect_success 'rev-list --objects includes trees' '
	git rev-list --objects HEAD >actual &&
	grep "subdir" actual
'

test_expect_success 'rev-list --objects with range' '
	git rev-list --objects first..HEAD >actual &&
	test $(wc -l <actual) -gt 3
'

test_expect_success 'rev-list --objects --all' '
	git rev-list --objects --all >actual &&
	test $(wc -l <actual) -gt 4
'

# --filter tests (all unsupported in grit)
test_expect_failure 'rev-list --filter=blob:none omits blobs' '
	git rev-list --objects --filter=blob:none HEAD >actual &&
	# Should list commits and trees but no blobs
	! grep "small.txt" actual &&
	! grep "large.bin" actual
'

test_expect_failure 'rev-list --filter=blob:none still lists commits' '
	git rev-list --objects --filter=blob:none HEAD >actual &&
	COMMIT=$(git rev-parse HEAD) &&
	grep "$COMMIT" actual
'

test_expect_failure 'rev-list --filter=tree:0 omits trees' '
	git rev-list --objects --filter=tree:0 HEAD >actual &&
	# Should not list any tree objects
	! grep "	$" actual
'

test_expect_failure 'rev-list --filter=blob:limit=100 omits large blobs' '
	git rev-list --objects --filter=blob:limit=100 HEAD >actual &&
	# large.bin is >100 bytes so should be filtered
	! grep "large.bin" actual &&
	# small.txt might still be there (13 bytes)
	grep "small.txt" actual
'

test_expect_failure 'rev-list --filter=blob:limit=1k keeps small files' '
	git rev-list --objects --filter=blob:limit=1k HEAD >actual &&
	grep "small.txt" actual
'

test_expect_failure 'rev-list --filter=blob:limit=1k omits large files' '
	git rev-list --objects --filter=blob:limit=1k HEAD >actual &&
	! grep "large.bin" actual
'

test_expect_failure 'rev-list --filter=combine:blob:none+tree:0 omits both' '
	git rev-list --objects --filter=combine:blob:none+tree:0 HEAD >actual &&
	# Should only list commit objects
	git rev-list HEAD >commits-only &&
	test_line_count = $(wc -l <commits-only | tr -d " ") actual
'

test_expect_failure 'rev-list --filter with --count' '
	git rev-list --filter=blob:none --count HEAD >actual &&
	test $(cat actual) -ge 1
'

test_expect_failure 'rev-list --objects --filter=blob:none with range' '
	git rev-list --objects --filter=blob:none first..HEAD >actual &&
	test $(wc -l <actual) -ge 1
'

# Basic rev-list features that DO work - ensure they work with the setup
test_expect_success 'rev-list HEAD lists all commits' '
	git rev-list HEAD >actual &&
	test_line_count = 4 actual
'

test_expect_success 'rev-list --count HEAD' '
	git rev-list --count HEAD >actual &&
	test $(cat actual) = 4
'

test_expect_success 'rev-list with tags as refs' '
	git rev-list first..fourth >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list --reverse HEAD' '
	git rev-list --reverse HEAD >actual &&
	FIRST=$(git rev-parse first) &&
	head -1 actual >got &&
	echo "$FIRST" >want &&
	test_cmp want got
'

test_expect_success 'rev-list --max-count=2 HEAD' '
	git rev-list --max-count=2 HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list --skip=1 HEAD' '
	git rev-list --skip=1 HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list --topo-order HEAD' '
	git rev-list --topo-order HEAD >actual &&
	test_line_count = 4 actual
'

test_expect_success 'rev-list --parents HEAD' '
	git rev-list --parents HEAD >actual &&
	# Each commit except root should have a parent
	tail -1 actual >root &&
	set -- $(cat root) &&
	test $# -eq 1
'

test_expect_success 'rev-list --all' '
	git rev-list --all >actual &&
	test_line_count = 4 actual
'

test_done
