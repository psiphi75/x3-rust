#!/bin/bash

#############################################################################
#                                                                           #
#   Rust implementation of the X3 lossless audio compression protocol.      #
#                                                                           #
#   Copyright (C) 2019 Simon M. Werner <simonwerner@gmail.com>              #
#                                                                           #
#   This program is free software; you can redistribute it and/or modify    #
#   it under the terms of the GNU General Public License as published by    #
#   the Free Software Foundation, either version 3 of the License, or       #
#   (at your option) any later version.                                     #
#                                                                           #
#   This program is distributed in the hope that it will be useful,         #
#   but WITHOUT ANY WARRANTY; without even the implied warranty of          #
#   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the            #
#   GNU General Public License for more details.                            #
#                                                                           #
#   You should have received a copy of the GNU General Public License       #
#   along with this program. If not, see <http://www.gnu.org/licenses/>.    #
#                                                                           #
############################################################################# 

#
# These tests will compress all the files in the given directory,
# decompress them, and test the original with the decompressed version.
# This validates end-to-end compression/decompression.
#

set -euo pipefail

FLAC="$(which flac) --totally-silent --force"
cargo build --release --bin x3 --features=std

TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" 0 2 3 15

X3=../target/release/x3
if [[ ! -x $X3 ]]; then
    echo "x3 binary not found at $X3"
    exit 1
fi

if [[ -z $(which hyperfine) ]]; then
  apt-get install -y hyperfine
fi
TIME="$(which hyperfine) --export-csv ${TEMP_DIR}/timing.csv --time-unit second"

command -v flac >/dev/null 2>&1 || { echo "flac is required"; exit 1; }
command -v $X3 >/dev/null 2>&1 || { echo "x3 is required"; exit 1; }

# Minimal portable workspace root (script is in test/)
THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SAMPLES_REPO="https://github.com/psiphi75/underwater-sound-samples"
CLONE_DIR="${THIS_DIR}/underwater-sound-samples"
SAMPLES_DIR="${CLONE_DIR}/samples"

# Ensure required tools
command -v git >/dev/null 2>&1 || { echo "git is required"; exit 1; }
command -v flac >/dev/null 2>&1 || { echo "flac is required"; exit 1; }


# Clone or pull the samples repo
if [ -d "${CLONE_DIR}/.git" ]; then
  git -C "${CLONE_DIR}" fetch --all --tags
  git -C "${CLONE_DIR}" pull --ff-only || git -C "${CLONE_DIR}" pull
else
  git clone --depth 1 "$SAMPLES_REPO" "${CLONE_DIR}"
fi



# Decompress .flac -> .wav
shopt -s nullglob
FLAC_FILES=(${SAMPLES_DIR}/*.flac)
if [ ${#FLAC_FILES[@]} -eq 0 ]; then
  echo "No .flac files found in ${SAMPLES_DIR}"
  exit 1
fi

for f in "${FLAC_FILES[@]}"; do
    base="$(basename "$f" .flac)"
    out="${SAMPLES_DIR}/${base}.wav"
    if [[ ! -f ${out} ]]; then
        flac -d -s -f -o "$out" "$f"
    fi
    cp "$out" $TEMP_DIR
done

function bench_wav_to_x3a {
    ${TIME} "${X3} --input $1 --output $2"
}

function bench_x3a_to_wav {
    ${TIME} "${X3} --input $1 --output $2"
}

function bench_wav_to_flac {
    ${TIME} "${FLAC} $1 --output-name=\"$2\""
}

function bench_flac_to_wav {
    ${TIME} "${FLAC} --decode $1 --output-name=\"$2\""
}

function bench_algo {
    local in_file_ext=$1
    local out_file_ext=$2
    local algorithm="${in_file_ext}_to_${out_file_ext}"
    local bench_sh="bench_${algorithm}"

    if [[ ${in_file_ext} == "wav" ]]; then
        local algo=${out_file_ext}
        local type="comp"
    else
        local algo=${in_file_ext}
        local type="dec"
    fi

    # Do the benchmark for all the audio files
    for in_base_file in $FILE_LIST
    do
        local in_file="${in_base_file}.${in_file_ext}"
        local out_file="${in_base_file}.${out_file_ext}"

        local orig_size=$(stat -c%s -- "${in_file}")
        Totalsize["$algo"|"$type"]=$((${Totalsize["$algo"|"$type"]} + ${orig_size}))

        # Run the benchmark
        local result="$(${bench_sh} ${in_file} ${out_file} 2>&1 > /dev/null)"
        local mean=$(awk -F, 'NR==2{ gsub(/"/,"",$2); print ($2+0); exit }' ${TEMP_DIR}/timing.csv)
        Totaltime["$algo"|"$type"]=$(echo "$mean + ${Totaltime[$algo|$type]}" | bc -l)
        local comp_size=$(stat -c%s -- "$out_file")

        # Choose size to use for MB/s: if input is wav use original size, else use compressed size
        if [ "${in_file_ext}" == "wav" ]; then
            Origsize[$algo]=$((Origsize[$algo] + "$orig_size"))
        fi
        
        echo "$(basename ${in_file}),${algorithm},${orig_size},${mean},${comp_size}"

    done
}

declare -A Totalsize
declare -A Totaltime
declare -A Origsize
Totalsize["x3a"|"dec"]=0
Totalsize["x3a"|"comp"]=0
Totalsize["flac"|"dec"]=0
Totalsize["flac"|"comp"]=0
Totaltime["x3a"|"dec"]=0
Totaltime["x3a"|"comp"]=0
Totaltime["flac"|"dec"]=0
Totaltime["flac"|"comp"]=0
Origsize["x3a"]=0
Origsize["x3a"]=0


FILE_LIST="$(find "${TEMP_DIR}" -name "*.wav" -print0 | while IFS= read -r -d '' f; do printf '%s\n' "${f%.wav}"; done)"
echo "File,Algorithm,File Size (B),Time,Max Mem Usage (kB),Compressed Size (B)"
bench_algo wav x3a
bench_algo x3a wav
bench_algo wav flac
bench_algo flac wav

echo -e "\n"

echo "Algorithm,Compression ratio,Compression speed (MB/s),Decompression speed (MB/s)"
for a in x3a flac; do
    declare -A mbps
    for t in comp dec; do
        mbps[$t]=$(echo "${Origsize[$a]}/${Totaltime[$a|$t]}/1024/1024" | bc -l)
    done
    comp_ratio=$(echo "${Totalsize[$a|'dec']}/${Totalsize[$a|'comp']}" | bc -l)

    echo "$a,${comp_ratio},${mbps['comp']},${mbps['dec']}"
done