#!/bin/sh
# Ported from git/t/t1001-read-tree-m-2way.sh.

test_description='grit read-tree -m two-way carry-forward'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Helper: two-way read-tree + ls-files
read_tree_twoway () {
	grit read-tree -m "$1" "$2" && grit ls-files --stage
}

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

# Create bozbar content files in trash
cat >bozbar-old <<\EOF
This is a sample file used in two-way fast-forward merge
tests.  Its second line ends with a magic word bozbar
which will be modified by the merged head to gnusto.
It has some extra lines so that external tools can
successfully merge independent changes made to later
lines (such as this one), avoiding line conflicts.
EOF

sed -e 's/bozbar/gnusto (earlier bozbar)/' bozbar-old >bozbar-new

test_expect_success 'setup' '
	grit init . &&
	echo frotz >frotz &&
	echo nitfol >nitfol &&
	cat bozbar-old >bozbar &&
	echo rezrov >rezrov &&
	echo yomin >yomin &&
	grit update-index --add nitfol bozbar rezrov &&
	grit write-tree >.treeH &&

	cat bozbar-new >bozbar &&
	grit update-index --add frotz bozbar &&
	grit update-index --force-remove rezrov &&
	grit ls-files --stage >M.out &&
	grit write-tree >.treeM
'

test_expect_success '4 - carry forward local addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	grit update-index --add yomin &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >4.out &&
	grep yomin 4.out &&
	check_cache_at yomin clean
'

test_expect_success '5 - carry forward local addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo yomin >yomin &&
	grit update-index --add yomin &&
	echo yomin yomin >yomin &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >5.out &&
	grep yomin 5.out &&
	check_cache_at yomin dirty
'

test_expect_success '6 - local addition already has the same.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	grit update-index --add frotz &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >6.out &&
	test_cmp M.out 6.out &&
	check_cache_at frotz clean
'

test_expect_success '7 - local addition already has the same.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo frotz >frotz &&
	grit update-index --add frotz &&
	echo frotz frotz >frotz &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >7.out &&
	test_cmp M.out 7.out &&
	check_cache_at frotz dirty
'

test_expect_success '8 - conflicting addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo frotz frotz >frotz &&
	grit update-index --add frotz &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '9 - conflicting addition.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo frotz frotz >frotz &&
	grit update-index --add frotz &&
	echo frotz >frotz &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '10 - path removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo rezrov >rezrov &&
	grit update-index --add rezrov &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >10.out &&
	test_cmp M.out 10.out
'

test_expect_success '12 - unmatching local changes being removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo rezrov rezrov >rezrov &&
	grit update-index --add rezrov &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '13 - unmatching local changes being removed.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo rezrov rezrov >rezrov &&
	grit update-index --add rezrov &&
	echo rezrov >rezrov &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '14 - unchanged in two heads.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo nitfol nitfol >nitfol &&
	grit update-index --add nitfol &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >14.out &&
	grep nitfol 14.out &&
	check_cache_at nitfol clean
'

test_expect_success '15 - unchanged in two heads.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo nitfol nitfol >nitfol &&
	grit update-index --add nitfol &&
	echo nitfol nitfol nitfol >nitfol &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >15.out &&
	grep nitfol 15.out &&
	check_cache_at nitfol dirty
'

test_expect_success '16 - conflicting local change.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo bozbar bozbar >bozbar &&
	grit update-index --add bozbar &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '17 - conflicting local change.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	echo bozbar bozbar >bozbar &&
	grit update-index --add bozbar &&
	echo bozbar bozbar bozbar >bozbar &&
	test_must_fail grit read-tree -m $treeH $treeM
'

test_expect_success '18 - local change already having a good result.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	cat bozbar-new >bozbar &&
	grit update-index --add bozbar &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >18.out &&
	test_cmp M.out 18.out &&
	check_cache_at bozbar clean
'

test_expect_success '19 - local change already having a good result, further modified.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	cat bozbar-new >bozbar &&
	grit update-index --add bozbar &&
	echo gnusto gnusto >bozbar &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >19.out &&
	test_cmp M.out 19.out &&
	check_cache_at bozbar dirty
'

test_expect_success '20 - no local change, use new tree.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	cat bozbar-old >bozbar &&
	grit update-index --add bozbar &&
	read_tree_twoway $treeH $treeM &&
	grit ls-files --stage >20.out &&
	test_cmp M.out 20.out &&
	check_cache_at bozbar dirty
'

test_expect_success '22 - local change cache updated.' '
	treeH=$(cat .treeH) && treeM=$(cat .treeM) &&
	rm -f .git/index &&
	grit read-tree $treeH &&
	grit checkout-index -u -f -q -a &&
	sed -e "s/such as/SUCH AS/" bozbar-old >bozbar &&
	grit update-index --add bozbar &&
	test_must_fail grit read-tree -m $treeH $treeM
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
	read_tree_twoway $treeDF $treeDFDF &&
	grit ls-files --stage >DFDFcheck.out &&
	test_cmp DFDF.out DFDFcheck.out
'

test_done
