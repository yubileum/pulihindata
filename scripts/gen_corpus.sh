#!/bin/bash
# Generate a test corpus of real files in many formats (needs ImageMagick,
# ffmpeg, 7z, rar, python3, mingw-w64). Output: $PULIH_TEST_DIR/corpus
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
S=${PULIH_TEST_DIR:-$HERE/../target/test-data}
mkdir -p "$S/corpus" && cd "$S/corpus" && rm -rf ./*
convert -size 1200x800 plasma:fractal -quality 90 photo1.jpg
convert -size 640x480 gradient:blue-yellow -quality 85 -interlace JPEG progressive.jpg
convert -size 800x600 plasma: photo2.png
convert -size 300x200 xc:red -fill white -draw "circle 150,100 150,10" anim.gif
convert -size 500x400 plasma: image.bmp
convert -size 400x300 plasma: -compress lzw image.tif
convert -size 640x480 plasma: image.webp
convert -size 300x300 plasma: image.psd
FF="ffmpeg -loglevel error -y"
$FF -f lavfi -i testsrc=size=640x360:rate=25 -f lavfi -i sine=frequency=440 -t 6 -c:v libx264 -pix_fmt yuv420p -c:a aac clip.mp4
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v libx264 -pix_fmt yuv420p -f mov clip.mov
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v libvpx -b:v 300k clip.webm
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v libx264 clip.mkv
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v mpeg4 clip.avi
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v flv1 clip.flv
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v wmv2 clip.wmv
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v mpeg2video clip.mpg
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v mpeg2video -f mpegts clip.ts
$FF -f lavfi -i testsrc=size=320x240:rate=25 -t 4 -c:v mpeg2video -f mpegts -mpegts_m2ts_mode 1 clip.mts
$FF -f lavfi -i sine=frequency=330 -t 8 -c:a libmp3lame -b:a 128k -metadata title=Lagu song.mp3
$FF -f lavfi -i sine=frequency=550 -t 5 tone.wav
$FF -f lavfi -i sine=frequency=660 -t 5 tone.flac
$FF -f lavfi -i sine=frequency=770 -t 5 -c:a libvorbis tone.ogg
$FF -f lavfi -i sine=frequency=880 -t 5 -c:a libopus voice.opus
$FF -f lavfi -i sine=frequency=990 -t 5 -c:a aac voice.m4a
python3 "$HERE/gen_docs.py"
7z a -bd -bso0 arsip.7z photo2.png song.mp3 >/dev/null
rar a -inul arsip.rar photo1.jpg image.bmp
printf '#include <stdio.h>\nint main(){puts("halo");return 0;}\n' > /tmp/pulih_h.c
x86_64-w64-mingw32-gcc -O2 /tmp/pulih_h.c -o program.exe
cp /usr/bin/true program.elf
ls -la
