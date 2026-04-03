#!/bin/sh

test_description='git apply with large patches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo
'

test_expect_success 'apply patch creating large file' '
	cd repo &&
	# Generate a patch that creates a file with many lines
	{
		echo "diff --git a/bigfile b/bigfile"
		echo "new file mode 100644"
		echo "--- /dev/null"
		echo "+++ b/bigfile"
		echo "@@ -0,0 +1,1000 @@"
		i=1
		while test $i -le 1000; do
			echo "+line $i"
			i=$(($i + 1))
		done
	} >big.patch &&
	git apply big.patch &&
	test -f bigfile &&
	test_line_count = 1000 bigfile
'

test_expect_success 'apply --stat on large patch' '
	cd repo &&
	git apply --stat big.patch >output &&
	test_grep "bigfile" output
'

test_expect_success 'apply --check on large patch validates' '
	cd repo &&
	rm -f bigfile &&
	git apply --check big.patch
'

test_done
