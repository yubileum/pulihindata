"""Assemble filesystem images into a partitioned disk image (GPT or MBR)."""
import struct, sys, uuid, zlib, os
def gpt(out, parts, total_mb):
    ss = 512; total = total_mb * 2048
    with open(out, 'wb') as f:
        f.truncate(total * ss)
        entries = bytearray(128 * 128)
        lba = 2048; layout = []
        for i, (img, name) in enumerate(parts):
            n = os.path.getsize(img) // ss
            e = uuid.UUID('EBD0A0A2-B9E5-4433-87C0-68B6B72699C7').bytes_le + uuid.uuid4().bytes_le
            e += struct.pack('<QQQ', lba, lba + n - 1, 0) + name.encode('utf-16le').ljust(72, b'\0')
            entries[i*128:(i+1)*128] = e
            layout.append((lba, img)); lba += n + 2048
        def header(cur, alt, ent_lba):
            h = bytearray(struct.pack('<8sIIIIQQQQ16sQIII', b'EFI PART', 0x10000, 92, 0, 0, cur, alt, 34, total - 34,
                                      uuid.uuid4().bytes_le, ent_lba, 128, 128, zlib.crc32(entries)))
            struct.pack_into('<I', h, 16, zlib.crc32(h[:92]))
            return bytes(h).ljust(512, b'\0')
        mbr = bytearray(512); mbr[446:462] = struct.pack('<BBBBBBBBII', 0, 0, 2, 0, 0xEE, 0xFF, 0xFF, 0xFF, 1, min(total - 1, 0xFFFFFFFF)); mbr[510:] = b'\x55\xAA'
        f.seek(0); f.write(mbr); f.write(header(1, total - 1, 2)); f.write(entries)
        f.seek((total - 33) * ss); f.write(entries); f.write(header(total - 1, 1, total - 33))
        for lba, img in layout:
            f.seek(lba * ss); f.write(open(img, 'rb').read())
def mbr(out, parts, total_mb):
    ss = 512
    with open(out, 'wb') as f:
        f.truncate(total_mb << 20)
        m = bytearray(512); lba = 2048
        for i, (img, ptype) in enumerate(parts):
            n = os.path.getsize(img) // ss
            m[446+16*i:462+16*i] = struct.pack('<BBBBBBBBII', 0, 0, 0, 0, ptype, 0, 0, 0, lba, n)
            f.seek(lba * ss); f.write(open(img, 'rb').read()); lba += n + 2048
        m[510:] = b'\x55\xAA'; f.seek(0); f.write(m)
kind, out, total = sys.argv[1], sys.argv[2], int(sys.argv[3])
items = [a.split('=') for a in sys.argv[4:]]
if kind == 'gpt': gpt(out, [(a, b) for a, b in items], total)
else: mbr(out, [(a, int(b, 16)) for a, b in items], total)
