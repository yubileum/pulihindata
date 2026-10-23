"""Write OOXML/ODF/ZIP/SQLite/PDF/RTF samples into the current directory."""
import os, random, sqlite3, zipfile
random.seed(7)
def ooxml(name, part):
    with zipfile.ZipFile(name, 'w', zipfile.ZIP_DEFLATED) as z:
        z.writestr('[Content_Types].xml', '<?xml version="1.0"?><Types/>')
        z.writestr('_rels/.rels', '<Relationships/>')
        z.writestr(part, '<doc>' + ' '.join(str(random.random()) for _ in range(4000)) + '</doc>')
ooxml('laporan.docx', 'word/document.xml')
ooxml('anggaran.xlsx', 'xl/workbook.xml')
ooxml('presentasi.pptx', 'ppt/presentation.xml')
with zipfile.ZipFile('arsip.zip', 'w', zipfile.ZIP_DEFLATED) as z:
    for i in range(20):
        z.writestr(f'data/file{i}.txt', os.urandom(3000).hex())
with zipfile.ZipFile('odt.odt', 'w') as z:
    z.writestr(zipfile.ZipInfo('mimetype'), 'application/vnd.oasis.opendocument.text')
    z.writestr('content.xml', '<x>' + 'teks ' * 2000 + '</x>')
db = sqlite3.connect('data.sqlite')
db.execute('create table t(a,b)')
db.executemany('insert into t values(?,?)', [(i, os.urandom(50).hex()) for i in range(3000)])
db.commit(); db.close()
objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R >>"]
stream = b"BT /F1 24 Tf 50 80 Td (Halo Pulih) Tj ET " + os.urandom(20000).hex().encode()
out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"); offs = []
for i, o in enumerate(objs, 1):
    offs.append(len(out)); out += f"{i} 0 obj\n{o}\nendobj\n".encode()
offs.append(len(out)); out += b"4 0 obj\n<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream\nendobj\n"
x = len(out); out += b"xref\n0 5\n0000000000 65535 f \n" + b"".join(b"%010d 00000 n \n" % o for o in offs)
out += b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % x
open('dokumen.pdf', 'wb').write(out)
open('surat.rtf', 'w').write(r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0\fs24 Halo {\b dunia} \'e9 " + "isi " * 3000 + "}")
