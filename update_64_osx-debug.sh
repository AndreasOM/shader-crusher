#!/bin/sh

path64="../SixtyFour/other/shadercrusher/"
cbindgen >shadercrusher.h 
cargo +nightly build --lib # --features="write-support"
cp shadercrusher.h ${path64}/shadercrusher.h 
cp target/debug/libshader_crusher.a ${path64}/osx/libshader_crusher.a
cp target/debug/libshader_crusher.dylib ${path64}/osx/libshader_crusher.dylib

