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

SOUND_DIR=$1
ALGORITHM=$2

TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" 0 2 3 15

function usage() {
    echo
    echo "Usage:"
    echo "  bench.sh DIRECTORY ALGORITHM"
    echo
    echo "Where:"
    echo "  DIRECTORY is the directory with the source files"
    echo "  ALGORITHM is the algorithm to test, one of: wav_to_x3a, x3a_to_wav, wav_to_flac, flac_to_wav"
    exit 1
}

if [ -z "${SOUND_DIR}" ] || [ ! -d ${SOUND_DIR} ] || [ -z "${ALGORITHM}" ]; then
    usage
fi

FLAC="$(which flac) --totally-silent --force"
X3=$(which x3)

X3=../target/release/x3

# This is not the usual bash/shell time command
TIME="$(which time) -f %e,%M"


case "${ALGORITHM}" in
    "wav_to_x3a")
        IN_FILE_EXT="wav"
        OUT_FILE_EXT="x3a"
        BENCH_SH=bench_wav_to_x3a
        ;;
    "x3a_to_wav")
        IN_FILE_EXT="x3a"
        OUT_FILE_EXT="wav"
        BENCH_SH=bench_x3a_to_wav
        ;;
    "wav_to_flac")
        IN_FILE_EXT="wav"
        OUT_FILE_EXT="flac"
        BENCH_SH=bench_wav_to_flac
        BENCH_PARAMS="--compression-level-0"
        ;;
    "flac_to_wav")
        IN_FILE_EXT="flac"
        OUT_FILE_EXT="wav"
        BENCH_SH=bench_flac_to_wav
        ;;
    *)
        usage
        exit 1
        ;;
esac



function bench_wav_to_x3a {
    ${TIME} ${X3} --input $1 --output $2
}

function bench_x3a_to_wav {
    ${TIME} ${X3} --input $1 --output $2
}

function bench_wav_to_flac {
    ${TIME} ${FLAC} $1 --output-name="$2" $3
}

function bench_flac_to_wav {
    ${TIME} ${FLAC} --decode $1 --output-name="$2"
}



echo "in file,test,in file size (bytes),test params,time,max mem usage (kB),out file size (bytes)"

for IN_FILE in $(ls ${SOUND_DIR}/*.${IN_FILE_EXT})
do

    # Get the file size, at the same time as loading the file to memory - hoping it stays there
    SIZE=$(cat ${IN_FILE} | wc -c)
    
    echo -n "$(basename ${IN_FILE}),${ALGORITHM},${SIZE},${BENCH_PARAMS},"

    # Export the timings
    OUT_FILE=${TEMP_DIR}/$(basename ${IN_FILE}).${OUT_FILE_EXT}

    # Get the benchmark results
    TIMING="$(${BENCH_SH} ${IN_FILE} ${OUT_FILE} ${BENCH_PARAMS}  2>&1 > /dev/null)"
    echo -n "${TIMING},"

    SIZE=$(cat ${OUT_FILE} | wc -c)
    echo ${SIZE}

    rm ${OUT_FILE}

done