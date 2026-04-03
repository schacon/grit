#!/bin/sh

test_description='git diagnose'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git commit --allow-empty -m "initial commit"
'

test_expect_success 'creates diagnostics report' '
	git diagnose &&
	ls git-diagnostics-*.txt >report_list &&
	test_line_count = 1 report_list &&
	test_file_not_empty "$(cat report_list)"
'

test_expect_success 'report contains version info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "grit diagnose" "$report" &&
	grep "git version" "$report"
'

test_expect_success 'report contains repository info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "\[Repository\]" "$report" &&
	grep "git_dir:" "$report"
'

test_expect_success 'report contains HEAD info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "\[HEAD\]" "$report"
'

test_expect_success 'report contains packs info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "\[Packs\]" "$report"
'

test_expect_success 'report contains loose objects info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "\[Loose Objects\]" "$report"
'

test_expect_success 'report contains config info' '
	report="$(ls git-diagnostics-*.txt | head -1)" &&
	grep "\[Config\]" "$report"
'

test_expect_success 'custom output path' '
	test_when_finished rm -f custom-diag.txt &&
	git diagnose -o custom-diag.txt &&
	test_path_is_file custom-diag.txt &&
	test_file_not_empty custom-diag.txt
'

test_expect_success 'diagnose after adding content' '
	echo "hello" >file.txt &&
	git add file.txt &&
	git commit -m "add file" &&
	git diagnose -o after-content-report.txt &&
	grep "entries:" after-content-report.txt
'

test_done
