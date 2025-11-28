#!/bin/bash

# Copied from: https://doc.rust-lang.org/rustc/profile-guided-optimization.html
#
# BEFORE
# Algorithm     Compression speed (MB/s)    Decompression speed (MB/s)
# x3a           105.98                      433.81
# flac          191.32                      243.84
#
# AFTER (with PGO)
# Algorithm     Compression speed (MB/s)    Decompression speed (MB/s)
# x3a           139.72                      384.61
# flac          184.69                      230.29

set -euo pipefail

# STEP 0: Start with a clean slate
TEMP_DIR=$(mktemp -d)
trap "rm -rf ${TEMP_DIR}" 0 2 3 15

# STEP 1: Build the instrumented binaries
RUSTFLAGS="-Cprofile-generate=${TEMP_DIR}" cargo build --release --bin x3 --features=std

# STEP 2: Run the instrumented binaries with some typical data
X3=../target/release/x3
for file in $(find . -name *.wav); do
    ${X3} --input $file --output $file.x3a > /dev/null
    ${X3} --input $file.x3a --output $file.x3a.wav > /dev/null
    rm $file.x3a $file.x3a.wav
    break
done

# STEP 3: Merge the `.profraw` files into a `.profdata` file
llvm-profdata-20 merge -o ${TEMP_DIR}/merged.profdata ${TEMP_DIR}

# STEP 4: Use the `.profdata` file for guiding optimizations
RUSTFLAGS="-Cprofile-use=${TEMP_DIR}/merged.profdata" cargo build --release --bin x3 --features=std

