#!/bin/bash

# Copied from: https://doc.rust-lang.org/rustc/profile-guided-optimization.html

# STEP 0: Make sure there is no left-over profiling data from previous runs
rm -rf /tmp/pgo-data

# STEP 1: Build the instrumented binaries
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release

# STEP 2: Run the instrumented binaries with some typical data
X3=../target/release/x3
${X3} --input files/EH120.wav --output x.x3a
${X3} --input x.x3a           --output w.wav
${X3} --input files/LI192.wav --output x.x3a
${X3} --input x.x3a           --output w.wav
${X3} --input files/PI240.wav --output x.x3a
${X3} --input x.x3a           --output w.wav
rm x.x3a w.wav

# STEP 3: Merge the `.profraw` files into a `.profdata` file
llvm-profdata merge -o /tmp/pgo-data/merged.profdata /tmp/pgo-data

# STEP 4: Use the `.profdata` file for guiding optimizations
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data/merged.profdata" cargo build --release
