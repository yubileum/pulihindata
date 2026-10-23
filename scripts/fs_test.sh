#!/bin/bash
# Usage: fs_test.sh ntfs|fat32|fat16|exfat [extra cli args]
# Builds a filesystem image, writes the corpus into folders, deletes files,
# then runs the scanner and recovers into out_<fs>.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
FS=$1; shift
S=${PULIH_TEST_DIR:-$HERE/../target/test-data}
C=$S/corpus; IMG=$S/$FS.img; M=$S/mnt_$FS; OUT=$S/out_$FS; W=$S/work_$FS
CLI=${PULIH_CLI:-$HERE/../target/release/pulih-cli}
rm -rf $IMG $M $OUT $W; mkdir -p $M $W
truncate -s 300M $IMG
# Stage the tree locally first.
mkdir -p $W/Dokumen/Kerja $W/Foto/Liburan $W/Musik $W/Video $W/tmp
cp $C/*.docx $C/*.xlsx $C/*.pdf $C/*.rtf $W/Dokumen/Kerja/
cp $C/*.jpg $C/*.png $C/*.gif $C/*.bmp $C/*.tif $C/*.psd $W/Foto/Liburan/
cp $C/*.mp3 $C/*.flac $C/*.ogg $C/*.wav $W/Musik/
cp $C/*.mp4 $C/*.mov $C/*.mkv $W/Video/
cp $C/arsip.zip "$W/Catatan rapat penting 2024.zip"
for i in $(seq 1 120); do head -c 65536 /dev/urandom > $W/tmp/s$i; done
head -c 3000000 /dev/urandom > $W/fragmen.bin
( cd $W && find . -type f \( -path './Dokumen/*' -o -path './Foto/*' -o -path './Musik/*' -o -name 'Catatan*' \) -exec md5sum {} + | sed 's#  \./#  #' ) > $S/expect_$FS.txt
( cd $W && md5sum fragmen.bin | sed 's#  #  Video/#' ) >> $S/expect_$FS.txt

case $FS in
fat32|fat16)
  [ $FS = fat16 ] && { truncate -s 0 $IMG; truncate -s 120M $IMG; mkfs.vfat -F 16 -n OLDCAM $IMG >/dev/null; } \
                  || mkfs.vfat -F 32 -n USBSTICK $IMG >/dev/null
  export MTOOLS_SKIP_CHECK=1
  I="-i $IMG"
  mmd $I ::/Dokumen ::/Dokumen/Kerja ::/Foto ::/Foto/Liburan ::/Musik ::/Video ::/tmp
  for d in Dokumen/Kerja Foto/Liburan Musik Video; do mcopy -m $I $W/$d/* ::/$d/; done
  mcopy -m $I "$W/Catatan rapat penting 2024.zip" ::/
  for i in $(seq 1 120); do mcopy $I $W/tmp/s$i ::/tmp/; done
  for i in $(seq 1 2 120); do mdel $I ::/tmp/s$i; done
  mcopy -m $I $W/fragmen.bin ::/Video/
  mdeltree $I ::/Foto/Liburan
  mdel $I ::/Dokumen/Kerja/dokumen.pdf ::/Dokumen/Kerja/laporan.docx "::/Catatan rapat penting 2024.zip" ::/Video/fragmen.bin
  for f in $(mdir -b $I ::/Musik); do mdel $I "$f"; done
  ;;
ntfs|exfat)
  if [ $FS = ntfs ]; then mkfs.ntfs -F -Q -L DataNTFS -c 4096 $IMG >/dev/null 2>&1; ntfs-3g $IMG $M
  else mkfs.exfat -L SDCARD $IMG >/dev/null; LOOP=$(losetup -f --show $IMG); mount.exfat-fuse $LOOP $M >/dev/null; fi
  cp -a $W/Dokumen $W/Foto $W/Musik $W/Video $W/tmp "$W/Catatan rapat penting 2024.zip" $M/
  sync
  for i in $(seq 1 2 120); do rm $M/tmp/s$i; done
  sync
  cp $W/fragmen.bin $M/Video/
  sync
  rm -rf $M/Foto/Liburan $M/Dokumen/Kerja/dokumen.pdf $M/Dokumen/Kerja/laporan.docx $M/Musik/* $M/Video/fragmen.bin "$M/Catatan rapat penting 2024.zip"
  sync
  cd /; fusermount -u $M 2>/dev/null || umount $M; sleep 1
  [ -n "$LOOP" ] && losetup -d $LOOP
  ;;
esac
$CLI scan $IMG --list --recover $OUT "$@" 2>&1 | grep -v ' tmp/' | tail -45
python3 $HERE/compare.py $S/expect_$FS.txt $OUT
