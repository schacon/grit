#!/bin/sh
# Ported subset from git/t/t1000-read-tree-m-3way.sh.

test_description='grit read-tree -m three-way basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup base/ours/theirs trees' '
	grit init repo &&
	cd repo &&
	echo base >shared &&
	echo same >same-change &&
	echo ours-base >ours-only &&
	echo theirs-base >theirs-only &&
	rm -f .git/index &&
	grit update-index --add shared same-change ours-only theirs-only &&
	tree_o=$(grit write-tree) &&
	echo "$tree_o" >../tree_o &&
	echo ours >shared &&
	echo same-final >same-change &&
	rm -f .git/index &&
	echo ours-base >ours-only &&
	grit update-index --add shared same-change ours-only &&
	tree_a=$(grit write-tree) &&
	echo "$tree_a" >../tree_a &&
	echo theirs >shared &&
	echo same-final >same-change &&
	rm -f .git/index &&
	echo theirs-base >theirs-only &&
	grit update-index --add shared same-change theirs-only &&
	tree_b=$(grit write-tree) &&
	echo "$tree_b" >../tree_b
'

test_expect_success 'three-way merge creates staged conflict for divergent path' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree -m "$(cat ../tree_o)" "$(cat ../tree_a)" "$(cat ../tree_b)" &&
	grit ls-files -u >actual &&
	test_path_is_file actual &&
	awk "\$3==1 && \$4==\"shared\" {c++} END {exit !(c==1)}" actual &&
	awk "\$3==2 && \$4==\"shared\" {c++} END {exit !(c==1)}" actual &&
	awk "\$3==3 && \$4==\"shared\" {c++} END {exit !(c==1)}" actual
'

test_expect_success 'three-way merge resolves identical changes to stage 0' '
	cd repo &&
	grit ls-files --stage same-change >actual &&
	! grep " 1	same-change$" actual &&
	! grep " 2	same-change$" actual &&
	! grep " 3	same-change$" actual &&
	grep " 0	same-change$" actual
'

test_done
