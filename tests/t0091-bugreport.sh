#!/bin/sh

test_description='git bugreport'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git commit --allow-empty -m "initial commit"
'

test_expect_success 'create a report with default name' '
	git bugreport &&
	ls git-bugreport-*.txt >report_list &&
	test_line_count = 1 report_list &&
	test_file_not_empty "$(cat report_list)"
'

test_expect_success 'report contains template sections' '
	report="$(ls git-bugreport-*.txt | head -1)" &&
	grep "\[System Info\]" "$report" &&
	grep "grit version" "$report" &&
	grep "\[What happened\]" "$report"
'

test_expect_success 'report contains system info' '
	report="$(ls git-bugreport-*.txt | head -1)" &&
	grep "grit version:" "$report"
'

test_expect_success 'create a report with custom output path' '
	test_when_finished rm -f custom-report.txt &&
	git bugreport -o custom-report.txt &&
	test_path_is_file custom-report.txt &&
	test_file_not_empty custom-report.txt
'

test_expect_failure 'dies if file with same name already exists' '
	test_when_finished rm -f existing-report.txt &&
	>existing-report.txt &&
	test_must_fail git bugreport -o existing-report.txt
'

test_expect_success 'runs outside of a git dir' '
	nongit git bugreport -o nongit-report.txt
'

test_done
