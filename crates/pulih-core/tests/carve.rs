//! End-to-end carving test on a synthetic in-memory disk: minimal but
//! structurally valid files of many formats are dropped between random
//! garbage, and every one must come back with its exact offset and size.

use std::sync::Arc;

use pulih_core::{start_scan, FileData, MemSource, Origin, ScanEvent, ScanOptions};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn png(rng: &mut Rng) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |ty: &[u8], data: &[u8]| {
        v.extend(be32(data.len() as u32));
        v.extend(ty);
        v.extend(data);
        v.extend(be32(0)); // CRC is not validated by the carver
    };
    let mut ihdr = Vec::new();
    ihdr.extend(be32(64));
    ihdr.extend(be32(48));
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &rng.bytes(5000));
    chunk(b"IDAT", &rng.bytes(3000));
    chunk(b"IEND", &[]);
    v
}

fn jpeg(rng: &mut Rng) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8];
    v.extend([0xFF, 0xE0, 0x00, 0x10]);
    v.extend(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    v.extend([0xFF, 0xDB, 0x00, 0x43, 0x00]);
    v.extend(rng.bytes(64));
    v.extend([
        0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
    ]);
    v.extend([0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    // Entropy-coded data: every 0xFF is stuffed with 0x00, plus a restart marker.
    for (i, b) in rng.bytes(20_000).into_iter().enumerate() {
        v.push(b);
        if b == 0xFF {
            v.push(0x00);
        }
        if i == 10_000 {
            v.extend([0xFF, 0xD3]);
        }
    }
    v.extend([0xFF, 0xD9]);
    v
}

fn gif(rng: &mut Rng) -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend([16, 0, 16, 0, 0x80, 0, 0]); // global colour table of 2 entries
    v.extend([0, 0, 0, 255, 255, 255]);
    v.extend([0x21, 0xF9, 4, 0, 0, 0, 0, 0]); // graphic control extension
    v.extend([0x2C, 0, 0, 0, 0, 16, 0, 16, 0, 0]);
    v.push(2); // LZW minimum code size
    for _ in 0..3 {
        v.push(200);
        v.extend(rng.bytes(200));
    }
    v.push(0);
    v.push(0x3B);
    v
}

fn bmp(rng: &mut Rng) -> Vec<u8> {
    let (w, h) = (40u32, 30u32);
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut v = b"BM".to_vec();
    v.extend(size.to_le_bytes());
    v.extend([0; 4]);
    v.extend(54u32.to_le_bytes());
    v.extend(40u32.to_le_bytes());
    v.extend(w.to_le_bytes());
    v.extend(h.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(24u16.to_le_bytes());
    v.extend([0; 24]);
    v.extend(rng.bytes((row * h) as usize));
    v
}

fn wav(rng: &mut Rng) -> Vec<u8> {
    let data = rng.bytes(12_000);
    let mut v = b"RIFF".to_vec();
    v.extend((36 + data.len() as u32).to_le_bytes());
    v.extend(b"WAVEfmt ");
    v.extend(16u32.to_le_bytes());
    v.extend([1, 0, 1, 0]);
    v.extend(8000u32.to_le_bytes());
    v.extend(16000u32.to_le_bytes());
    v.extend([2, 0, 16, 0]);
    v.extend(b"data");
    v.extend((data.len() as u32).to_le_bytes());
    v.extend(data);
    v
}

fn pdf(rng: &mut Rng) -> Vec<u8> {
    let mut v =
        b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n2 0 obj\n<< /Length 4000 >>\nstream\n"
            .to_vec();
    v.extend(rng.bytes(4000));
    v.extend(b"\nendstream\nendobj\nxref\n0 3\ntrailer\n<< /Root 1 0 R >>\nstartxref\n9\n%%EOF\n");
    // Incremental update: a second revision must be included.
    v.extend(b"3 0 obj\n<< >>\nendobj\nxref\ntrailer\n<< >>\nstartxref\n99\n%%EOF\n");
    v
}

fn zip(rng: &mut Rng) -> Vec<u8> {
    let data = rng.bytes(7000);
    let name = b"word/document.xml";
    let ct = b"[Content_Types].xml";
    let mut v = Vec::new();
    let mut cd = Vec::new();
    for (n, d) in [(&ct[..], &b"<Types/>"[..]), (&name[..], &data[..])] {
        let off = v.len() as u32;
        v.extend(b"PK\x03\x04");
        v.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        v.extend((d.len() as u32).to_le_bytes());
        v.extend((d.len() as u32).to_le_bytes());
        v.extend((n.len() as u16).to_le_bytes());
        v.extend([0, 0]);
        v.extend(n);
        v.extend(d);
        cd.extend(b"PK\x01\x02");
        cd.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        cd.extend((d.len() as u32).to_le_bytes());
        cd.extend((d.len() as u32).to_le_bytes());
        cd.extend((n.len() as u16).to_le_bytes());
        cd.extend([0; 12]);
        cd.extend(off.to_le_bytes());
        cd.extend(n);
    }
    let cd_off = v.len() as u32;
    v.extend(&cd);
    v.extend(b"PK\x05\x06\0\0\0\0\x02\0\x02\0");
    v.extend((cd.len() as u32).to_le_bytes());
    v.extend(cd_off.to_le_bytes());
    v.extend([0, 0]);
    v
}

fn mp4(rng: &mut Rng) -> Vec<u8> {
    let mut v = Vec::new();
    let mut boxed = |ty: &[u8], body: &[u8]| {
        v.extend(be32(8 + body.len() as u32));
        v.extend(ty);
        v.extend(body);
    };
    boxed(b"ftyp", b"isom\0\0\x02\0isomiso2avc1mp41");
    boxed(b"free", &[]);
    boxed(b"mdat", &rng.bytes(30_000));
    boxed(b"moov", &rng.bytes(900));
    v
}

#[test]
fn carves_every_format_exactly() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let samples: Vec<(&str, Vec<u8>)> = vec![
        ("png", png(&mut rng)),
        ("jpg", jpeg(&mut rng)),
        ("gif", gif(&mut rng)),
        ("bmp", bmp(&mut rng)),
        ("wav", wav(&mut rng)),
        ("pdf", pdf(&mut rng)),
        ("docx", zip(&mut rng)),
        ("mp4", mp4(&mut rng)),
    ];
    let mut disk = rng.bytes(1 << 20);
    let mut expect = Vec::new();
    for (ext, data) in &samples {
        let n = 20_000 + (rng.next() % 50_000) as usize;
        let gap = rng.bytes(n);
        disk.extend(gap);
        while !disk.len().is_multiple_of(4096) {
            disk.push(rng.next() as u8);
        }
        expect.push((*ext, disk.len() as u64, data.len() as u64));
        disk.extend(data);
    }
    disk.extend(rng.bytes(1 << 20));

    let opts = ScanOptions {
        quick: false,
        free_only: false,
        ..Default::default()
    };
    let h = start_scan(Arc::new(MemSource(disk)), opts);
    let mut carved = Vec::new();
    for ev in h.events.iter() {
        match ev {
            ScanEvent::CarveFinal(v) => carved = v,
            ScanEvent::Done(_) => break,
            _ => {}
        }
    }
    for (ext, off, len) in &expect {
        let hit = carved.iter().find(|f| f.first_offset() == *off);
        let hit = hit.unwrap_or_else(|| panic!("{ext} at {off} not carved"));
        assert_eq!(hit.ext, *ext, "type of file at {off}");
        assert_eq!(hit.size, *len, "{ext} size");
        assert_eq!(hit.origin, Origin::Carved);
        assert!(matches!(&hit.data, FileData::Extents(e) if e.len() == 1));
    }
    assert_eq!(
        carved.len(),
        expect.len(),
        "no false positives in random data: {carved:#?}"
    );
}
