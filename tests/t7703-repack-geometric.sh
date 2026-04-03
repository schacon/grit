#!/bin/sh

test_description='git repack operations (grit verification)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repack-geo &&
	cd repack-geo &&

	for i in $(test_seq 1 5); do
		echo "content $i" >file$i &&
		git add file$i &&
		test_tick &&
		git commit -m "commit $i" || return 1
	done
'

test_expect_success 'grit reads repo with loose objects' '
	cd repack-geo &&
	git log --oneline >output &&
	test $(wc -l <output) -eq 5
'

test_expect_success 'grit cat-file on loose objects' '
	cd repack-geo &&
	git cat-file -t HEAD >output &&
	echo commit >expect &&
	test_cmp expect output
'

test_expect_success 'grit ls-tree on loose objects' '
	cd repack-geo &&
	git ls-tree HEAD >output &&
	test $(wc -l <output) -eq 5
'

test_expect_success 'grit repack creates pack files' '
	cd repack-geo &&
	git repack >output 2>&1 &&
	ls .git/objects/pack/*.pack >packs &&
	test -s packs
'

test_done
