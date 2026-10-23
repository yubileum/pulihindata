"""Compare recovered files against the md5 list taken before deletion."""
import hashlib, os, sys
exp_file, out = sys.argv[1], sys.argv[2]
deleted_only = len(sys.argv) > 3
expect = {}
for line in open(exp_file):
    md5, path = line.rstrip('\n').split('  ', 1)
    expect[path] = md5
got = {}
for root, _, files in os.walk(out):
    for n in files:
        p = os.path.join(root, n)
        rel = os.path.relpath(p, out).split(os.sep, 1)[1]  # strip volume folder
        got[rel] = hashlib.md5(open(p, 'rb').read()).hexdigest()
def fat_alias(p):
    # FAT loses the first character of deleted 8.3 names ("photo1.jpg" -> "_hoto1.jpg").
    d, _, n = p.rpartition('/')
    return (d + '/' if d else '') + '_' + n[1:]
for p in list(expect):
    if p not in got and fat_alias(p) in got:
        got[p] = got.pop(fat_alias(p))
ok = [p for p in expect if got.get(p) == expect[p]]
bad = [p for p in expect if p in got and got[p] != expect[p]]
missing = [p for p in expect if p not in got]
print(f'exact: {len(ok)}  mismatched: {len(bad)}  not recovered: {len(missing)}')
for p in bad: print('  MISMATCH', p)
for p in missing: print('  MISSING ', p)
