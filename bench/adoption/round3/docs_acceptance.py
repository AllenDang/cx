"""Negative control: documentation-only changes must remain documentation-only."""
import pathlib
import subprocess

NOTE = 'Contributor note: documentation-only changes do not alter cache behavior.'
GIT = '/Library/Developer/CommandLineTools/usr/bin/git'
original = subprocess.check_output([GIT, 'show', 'HEAD:README.rst'], text=True)
current = pathlib.Path('README.rst').read_text()
assert current.rstrip() == original.rstrip() + '\n\n' + NOTE
assert current.count(NOTE) == 1
changed = subprocess.check_output([GIT, 'diff', '--name-only', 'HEAD'], text=True).splitlines()
assert changed == ['README.rst'], changed
print('Documentation control passed')
