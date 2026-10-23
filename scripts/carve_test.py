"""Build a raw image with every corpus file at a cluster-aligned offset,
separated by random garbage, then check the carver recovers each one
byte-exactly."""
import hashlib, os, random, subprocess, sys, shutil, re
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
S = os.environ.get('PULIH_TEST_DIR', os.path.join(ROOT, 'target', 'test-data'))
corpus = os.path.join(S, 'corpus')
img = os.path.join(S, 'carve.img')
out = os.path.join(S, 'carve_out')
cli = os.environ.get('PULIH_CLI', os.path.join(ROOT, 'target', 'release', 'pulih-cli'))
random.seed(1)
files = sorted(os.listdir(corpus))
expect = {}
with open(img, 'wb') as f:
    f.write(os.urandom(1 << 20))
    for i, name in enumerate(files):
        data = open(os.path.join(corpus, name), 'rb').read()
        # garbage gap: random bytes, sometimes zeros
        gap = random.choice([4096, 8192, 65536, 12288])
        f.write(os.urandom(gap) if i % 3 else b'\0' * gap)
        pad = (-f.tell()) % 4096
        if os.environ.get('UNALIGNED'):
            pad += random.randint(1, 511)
        f.write(os.urandom(pad))
        expect[f.tell()] = (name, hashlib.md5(data).hexdigest(), len(data))
        f.write(data)
    f.write(os.urandom(1 << 20))
shutil.rmtree(out, ignore_errors=True)
args = [cli, 'scan', img, '--deep-only', '--all-space', '--list', '--recover', out] + sys.argv[1:]
r = subprocess.run(args, capture_output=True, text=True)
print(r.stderr)
# Map each carved result's offset to its recovered file. Names may come from
# the content (document title, song tag), so go through the --list output.
listed = {}
for line in r.stdout.splitlines():
    m = re.match(r'^Signature\s+\S+\s+\S+ \S+\s+(?:-|\S+ \S+)\s+(.*?)\s+(\d+)(?:  \[.*\])?$', line)
    if m:
        listed[int(m.group(2))] = m.group(1)
files_by_name = {}
for root, _, fs in os.walk(out):
    for n in fs:
        files_by_name.setdefault(n, []).append(os.path.join(root, n))
found = {}
for off, name in listed.items():
    cands = files_by_name.get(os.path.basename(name), [])
    if cands:
        found[off] = cands.pop(0)
ok = bad = 0
for off, (name, md5, size) in sorted(expect.items()):
    p = found.get(off)
    if not p:
        print(f'MISSING  {name:20} @ {off}'); bad += 1; continue
    got = open(p, 'rb').read()
    if hashlib.md5(got).hexdigest() == md5:
        ok += 1; print(f'OK       {name:20} -> {os.path.basename(p)}')
    else:
        bad += 1; print(f'MISMATCH {name:20} size {size} got {len(got)} ({os.path.basename(p)})')
extra = set(found) - set(expect)
print(f'\n{ok}/{len(expect)} exact, {bad} bad, {len(extra)} extra carved files')
for e in sorted(extra)[:20]:
    print('  extra:', os.path.basename(found[e]), os.path.getsize(found[e]))
