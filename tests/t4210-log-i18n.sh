#!/bin/sh
# Tests for 'grit log' with i18n / non-ASCII content.

test_description='grit log with i18n and encoding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Unset test harness author/committer vars so git config values are used
unset GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

test_expect_success 'setup repo with UTF-8 commit messages' '
	git init repo &&
	cd repo &&
	git config user.name "Tëst Üser" &&
	git config user.email "test@example.com" &&

	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m "première modification" &&

	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m "zweite Änderung" &&

	echo three >file &&
	git add file &&
	test_tick &&
	git commit -m "третье изменение" &&

	echo four >file &&
	git add file &&
	test_tick &&
	git commit -m "四番目の変更" &&

	echo five >file &&
	git add file &&
	test_tick &&
	git commit -m "다섯 번째 변경" &&

	echo six >file &&
	git add file &&
	test_tick &&
	git commit -m "sjätte ändringen"
'

test_expect_success 'log --oneline shows UTF-8 subjects' '
	cd repo &&
	git log --oneline --no-decorate >actual &&
	grep "sjätte ändringen" actual &&
	grep "다섯 번째 변경" actual &&
	grep "四番目の変更" actual &&
	grep "третье изменение" actual &&
	grep "zweite Änderung" actual &&
	grep "première modification" actual
'

test_expect_success 'log --format=%s preserves UTF-8 subjects' '
	cd repo &&
	git log --format="%s" >actual &&
	cat >expect <<-\EOF &&
	sjätte ändringen
	다섯 번째 변경
	四番目の変更
	третье изменение
	zweite Änderung
	première modification
	EOF
	test_cmp expect actual
'

test_expect_success 'log --format=%an shows UTF-8 author name' '
	cd repo &&
	git log -n1 --format="%an" >actual &&
	echo "Tëst Üser" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%an for all commits shows same author' '
	cd repo &&
	git log --format="%an" >actual &&
	cat >expect <<-\EOF &&
	Tëst Üser
	Tëst Üser
	Tëst Üser
	Tëst Üser
	Tëst Üser
	Tëst Üser
	EOF
	test_cmp expect actual
'

test_expect_success 'log -n limits output with UTF-8 content' '
	cd repo &&
	git log -n 2 --format="%s" >actual &&
	test_line_count = 2 actual &&
	head -1 actual >first &&
	echo "sjätte ändringen" >expect &&
	test_cmp expect first
'

test_expect_success 'log --reverse with UTF-8 content' '
	cd repo &&
	git log --reverse --format="%s" >actual &&
	head -1 actual >first &&
	echo "première modification" >expect &&
	test_cmp expect first
'

test_expect_success 'log --skip with UTF-8 content' '
	cd repo &&
	git log --skip=4 --format="%s" >actual &&
	test_line_count = 2 actual &&
	head -1 actual >first &&
	echo "zweite Änderung" >expect &&
	test_cmp expect first
'

test_expect_success 'setup repo with UTF-8 file paths' '
	cd repo &&
	echo café >café.txt &&
	git add café.txt &&
	test_tick &&
	git commit -m "add café file" &&

	echo naïve >naïve.txt &&
	git add naïve.txt &&
	test_tick &&
	git commit -m "add naïve file"
'

test_expect_success 'log shows commits with UTF-8 filenames' '
	cd repo &&
	git log -n 2 --format="%s" >actual &&
	grep "café" actual &&
	grep "naïve" actual
'

test_expect_success 'setup author with CJK name' '
	cd repo &&
	git config user.name "田中太郎" &&
	echo japan >jp &&
	git add jp &&
	test_tick &&
	git commit -m "日本語のコミット"
'

test_expect_success 'log --format=%an shows CJK author' '
	cd repo &&
	git log -n1 --format="%an" >actual &&
	echo "田中太郎" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%s shows CJK subject' '
	cd repo &&
	git log -n1 --format="%s" >actual &&
	echo "日本語のコミット" >expect &&
	test_cmp expect actual
'

test_expect_success 'setup commit with emoji in message' '
	cd repo &&
	git config user.name "Test User" &&
	echo emoji >emoji &&
	git add emoji &&
	test_tick &&
	git commit -m "🚀 rocket launch 🎉"
'

test_expect_success 'log shows emoji in subject' '
	cd repo &&
	git log -n1 --format="%s" >actual &&
	echo "🚀 rocket launch 🎉" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --oneline shows emoji' '
	cd repo &&
	git log -n1 --oneline --no-decorate >actual &&
	grep "🚀 rocket launch 🎉" actual
'

test_expect_success 'setup commit with multi-line UTF-8 body' '
	cd repo &&
	cat >msg <<-\EOF &&
	Résumé des changements

	Première ligne du corps
	Deuxième ligne avec des accents: àéîõü
	Troisième ligne avec des symboles: ñ ü ö ä
	EOF
	echo body >body &&
	git add body &&
	test_tick &&
	git commit -F msg
'

test_expect_success 'log --format=%s shows UTF-8 subject from multi-line' '
	cd repo &&
	git log -n1 --format="%s" >actual &&
	echo "Résumé des changements" >expect &&
	test_cmp expect actual
'

test_expect_success 'setup commits with mixed scripts in author' '
	cd repo &&
	git config user.name "Москва User" &&
	echo mix1 >mix1 &&
	git add mix1 &&
	test_tick &&
	git commit -m "mixed Cyrillic author" &&

	git config user.name "مستخدم عربي" &&
	echo mix2 >mix2 &&
	git add mix2 &&
	test_tick &&
	git commit -m "Arabic author name"
'

test_expect_success 'log --format=%an shows Cyrillic author' '
	cd repo &&
	git log --skip=1 -n1 --format="%an" >actual &&
	echo "Москва User" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%an shows Arabic author' '
	cd repo &&
	git log -n1 --format="%an" >actual &&
	echo "مستخدم عربي" >expect &&
	test_cmp expect actual
'

test_expect_success 'log with --format combining UTF-8 fields' '
	cd repo &&
	git log -n1 --format="%an <%ae>: %s" >actual &&
	echo "مستخدم عربي <test@example.com>: Arabic author name" >expect &&
	test_cmp expect actual
'

test_expect_success 'setup commit with special UTF-8 punctuation' '
	cd repo &&
	git config user.name "Test User" &&
	echo special >special &&
	git add special &&
	test_tick &&
	cat >cmsg <<-\EOF &&
	em-dash—and ellipsis… here
	EOF
	git commit -F cmsg
'

test_expect_success 'log preserves typographic punctuation' '
	cd repo &&
	git log -n1 --format="%s" >actual &&
	cat >expect <<-\EOF &&
	em-dash—and ellipsis… here
	EOF
	test_cmp expect actual
'

test_expect_success 'setup commit with accented Latin characters' '
	cd repo &&
	git config user.name "José García López" &&
	echo spanish >spanish &&
	git add spanish &&
	test_tick &&
	git commit -m "añadir archivo español"
'

test_expect_success 'log shows accented Latin in author and subject' '
	cd repo &&
	git log -n1 --format="%an: %s" >actual &&
	echo "José García López: añadir archivo español" >expect &&
	test_cmp expect actual
'

test_expect_success 'log total count is correct' '
	cd repo &&
	git log --format="%s" >actual &&
	test_line_count = 15 actual
'

test_expect_success 'log --reverse first entry is original UTF-8 commit' '
	cd repo &&
	git log --reverse --format="%s" >actual &&
	head -1 actual >first &&
	echo "première modification" >expect &&
	test_cmp expect first
'

test_expect_success 'log --reverse contains all commits' '
	cd repo &&
	git log --reverse --format="%s" >actual &&
	test_line_count = 15 actual &&
	grep "première modification" actual &&
	grep "añadir archivo español" actual
'

test_done
