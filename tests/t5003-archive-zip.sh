#!/bin/sh

test_description='git archive --format=zip test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "file content" >file.txt &&
	mkdir subdir &&
	echo "subdir content" >subdir/nested.txt &&
	git add file.txt subdir/nested.txt &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'archive --format=zip produces output' '
	git archive --format=zip HEAD >archive.zip &&
	test -s archive.zip
'

test_expect_success 'zip archive has correct magic bytes' '
	# ZIP files start with PK\003\004
	head -c 2 archive.zip >magic &&
	printf "PK" >expect_magic &&
	test_cmp_bin expect_magic magic
'

test_expect_success 'zip archive contains correct files (python)' '
	python3 -c "
import zipfile, sys
z = zipfile.ZipFile(\"archive.zip\")
names = sorted(z.namelist())
for n in names:
    print(n)
" >zip_contents &&
	grep "file.txt" zip_contents &&
	grep "subdir/nested.txt" zip_contents
'

test_expect_success 'zip archive with --prefix' '
	git archive --format=zip --prefix=proj/ HEAD >prefix.zip &&
	python3 -c "
import zipfile
z = zipfile.ZipFile(\"prefix.zip\")
names = sorted(z.namelist())
for n in names:
    print(n)
" >prefix_contents &&
	grep "proj/file.txt" prefix_contents
'

test_expect_success 'zip archive file content is correct' '
	python3 -c "
import zipfile
z = zipfile.ZipFile(\"archive.zip\")
content = z.read(\"file.txt\").decode()
print(content, end=\"\")
" >zip_file_content &&
	echo "file content" >expect &&
	test_cmp expect zip_file_content
'

test_done
