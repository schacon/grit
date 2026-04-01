#!/bin/sh
# Ported from git/t/t1002-read-tree-m-u-2way.sh.

test_description='grit read-tree -m -u two-way updates worktree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Helper: check whether a path is clean or dirty in the cache
check_cache_at () {
	grit diff-files -- "$1" >_cca_out &&
	_cca_empty=$(cat _cca_out) &&
	case "$_cca_empty" in
	"")  echo "$1: clean" ;;
	?*)  echo "$1: dirty" ;;
	esac &&
	case "$2,$_cca_empty" in
	clean,)		:     ;;
	clean,?*)	false ;;
	dirty,)		false ;;
	dirty,?*)	:     ;;
	esac
}

test_expect_success 'setup' '
	grit init . &&
	echo frotz >frotz &&
	echo nitfol >nitfol &&
	echo bozbar >bozbar &&
	echo rezrov >rezrov &&
	grit update-index --add nitfol bozbar rezrov &&
	grit write-tree >.treeH &&

	echo gnusto >bozbar &&
	grit update-index --add frotz bozbar &&
	grit update-index --force-remove rezrov &&
	grit ls-files --stage >M.out &&
	grit write-tree >.treeM &&
	cp bozbar bozbar.M &&
	cp frotz frotz.M &&
	cp nitfol nitfol.M
'

test_expect_success '1, 2, 3 - no carry forward' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >1-3.out &&
	cmp M.out 1-3.out &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol &&
	check_cache_at bozbar clean &&
	check_cache_at frotz clean &&
	check_cache_at nitfol clean
'

test_expect_success '4 - carry forward local addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo yomin >yomin &&
	grit update-index --add yomin &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >4.out &&
	grep yomin 4.out &&
	check_cache_at yomin clean &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol &&
	echo yomin >yomin1 &&
	diff yomin yomin1 &&
	rm -f yomin1
'



test_expect_success '6 - local addition already has the same.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo frotz >frotz &&
	grit update-index --add frotz &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >6.out &&
	test_cmp M.out 6.out &&
	check_cache_at frotz clean &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol
'



test_expect_success '8 - conflicting addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo frotz frotz >frotz &&
	grit update-index --add frotz &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '9 - conflicting addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo frotz frotz >frotz &&
	grit update-index --add frotz &&
	echo frotz >frotz &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '10 - path removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo rezrov >rezrov &&
	grit update-index --add rezrov &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >10.out &&
	cmp M.out 10.out &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol
'

test_expect_success '12 - unmatching local changes being removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo rezrov rezrov >rezrov &&
	grit update-index --add rezrov &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '13 - unmatching local changes being removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo rezrov rezrov >rezrov &&
	grit update-index --add rezrov &&
	echo rezrov >rezrov &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '14 - unchanged in two heads.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo nitfol nitfol >nitfol &&
	grit update-index --add nitfol &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >14.out &&
	grep nitfol 14.out &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	check_cache_at nitfol clean
'

test_expect_success '15 - unchanged in two heads.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo nitfol nitfol >nitfol &&
	grit update-index --add nitfol &&
	echo nitfol nitfol nitfol >nitfol &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >15.out &&
	grep nitfol 15.out &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz
'

test_expect_success '16 - conflicting local change.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo bozbar bozbar >bozbar &&
	grit update-index --add bozbar &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '17 - conflicting local change.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo bozbar bozbar >bozbar &&
	grit update-index --add bozbar &&
	echo bozbar bozbar bozbar >bozbar &&
	test_must_fail grit read-tree -m -u $treeH $treeM
'

test_expect_success '18 - local change already having a good result.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo gnusto >bozbar &&
	grit update-index --add bozbar &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >18.out &&
	test_cmp M.out 18.out &&
	check_cache_at bozbar clean &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol
'



test_expect_success '20 - no local change, use new tree.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index nitfol bozbar rezrov frotz &&
	grit read-tree --reset -u $treeH &&
	echo bozbar >bozbar &&
	grit update-index --add bozbar &&
	grit read-tree -m -u $treeH $treeM &&
	grit ls-files --stage >20.out &&
	test_cmp M.out 20.out &&
	check_cache_at bozbar clean &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol
'

test_expect_success 'DF vs DF/DF case setup.' '
	rm -f .git/index &&
	echo DF >DF &&
	grit update-index --add DF &&
	grit write-tree >.treeDF &&

	rm -f DF &&
	mkdir DF &&
	echo DF/DF >DF/DF &&
	grit update-index --add DF/DF &&
	grit update-index --remove DF &&
	grit write-tree >.treeDFDF &&
	grit ls-files --stage >DFDF.out
'

test_expect_success 'DF vs DF/DF case test.' '
	treeDF=$(cat .treeDF) && treeDFDF=$(cat .treeDFDF) &&
	rm -f .git/index &&
	rm -fr DF &&
	echo DF >DF &&
	grit update-index --add DF &&
	grit read-tree -m -u $treeDF $treeDFDF &&
	grit ls-files --stage >DFDFcheck.out &&
	test_cmp DFDF.out DFDFcheck.out &&
	check_cache_at DF/DF clean
'

test_done
