#!/bin/sh

path64="../../../SixtyFour/other/shadercrusher/"
cbindgen >shadercrusher.h 
cargo +nightly build # --features="write-support"
cp shadercrusher.h ${path64}/ceshadercrusherreal.h 
cp target/debug/libshadercrusher.a ${path64}/osx/libshadercrusher.a

