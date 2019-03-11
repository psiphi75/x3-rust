#!/bin/sh

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

X3=../target/release/x3
W2S=../target/release/wav_to_str

SOUND_DIR=$1
if [ -z ${SOUND_DIR} ] || [ ! -d ${SOUND_DIR} ]; then
  echo
  echo "Usage:"
  echo "  test_wavs.sh [DIRECTORY]"
  echo
  exit 1
fi

# build it
cargo build --release

TEMP_X3A=$(mktemp).x3a
trap "rm -f $TEMP_X3A" 0 2 3 15

TEMP_WAV=$(mktemp).wav
trap "rm -f $TEMP_WAV" 0 2 3 15

TEMP_WAV_STR_ORIG=${TEMP_WAV}.raw-orig
trap "rm -f $TEMP_WAV_STR_ORIG" 0 2 3 15

TEMP_WAV_STR_TEST=${TEMP_WAV}.raw-orig
trap "rm -f $TEMP_WAV_STR_TEST" 0 2 3 15


SOUNDS=$(ls $SOUND_DIR/*.wav)
for WAV in ${SOUNDS}
do

  echo "Testing $WAV"

  echo "  Encoding to ${TEMP_X3A}"
  $X3 --input $WAV --output $TEMP_X3A  > /dev/null

  echo "  Decoding ${TEMP_X3A} to ${TEMP_WAV}"
  $X3 --input $TEMP_X3A --output $TEMP_WAV > /dev/null

  echo "  Checking"
  $W2S --wav $WAV > "${TEMP_WAV_STR_ORIG}"
  $W2S --wav $TEMP_WAV > "${TEMP_WAV_STR_TEST}"
  WAV_DIFF=$(diff "${TEMP_WAV_STR_ORIG}" "${TEMP_WAV_STR_TEST}")
  if [ -n "${WAV_DIFF}" ]; then
    echo "  TEST FAILED"
    echo ${WAV_DIFF}
    exit 1
  fi

  echo "  Ok"
  echo

done

