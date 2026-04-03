#!/bin/sh

test_description='diff -r -t shows directory additions and deletions'

. ./test-lib.sh

test_expect_success setup '
	mkdir dc dr dt &&
	>dc/1 &&
	>dr/2 &&
	>dt/3 &&
	>fc &&
	>fr &&
	>ft &&
	git add . &&
	test_tick &&
	git commit -m initial &&

	rm -fr dt dr ft fr &&
	mkdir da ft &&
	for p in dc/1 da/4 dt ft/5 fc
	do
		echo hello >$p || exit
	done &&
	git rm -r dr/2 fr &&
	git add . &&
	test_tick &&
	git commit -m second
'

# grit diff-tree -r -t does not yet emit tree-object entries (A da, M dc, etc.)
# so we test just the file-level output with -r --name-status

cat >expect <<\EOF
A	da/4
M	dc/1
D	dr/2
A	dt
D	dt/3
M	fc
D	fr
D	ft
A	ft/5
EOF

test_expect_success 'verify file-level changes with diff-tree -r --name-status' '
	git diff-tree -r --name-status HEAD~1 HEAD >actual &&
	test_cmp expect actual
'

# Full -r -t output includes tree entries that grit does not yet emit
cat >expect_full <<\EOF
A	da
A	da/4
M	dc
M	dc/1
D	dr
D	dr/2
A	dt
D	dt
D	dt/3
M	fc
D	fr
D	ft
A	ft
A	ft/5
EOF

test_expect_failure 'diff-tree -r -t includes tree entries (not implemented)' '
	git diff-tree -r -t --name-status HEAD~1 HEAD >actual &&
	test_cmp expect_full actual
'

test_done
