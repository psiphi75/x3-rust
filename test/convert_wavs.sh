#!/bin/sh

X3=../target/release/x3
SOUND_DIR=$1

# build it
cargo build --release

TEMP_X3A=$(mktemp).x3a
trap "rm -f $TEMP_X3A" 0 2 3 15

TEMP_WAV=$(mktemp).wav
trap "rm -f $TEMP_WAV" 0 2 3 15


SOUNDS=$(ls $SOUND_DIR/*.wav)
for WAV in ${SOUNDS}
do

  echo "Encoding ${WAV} to ${TEMP_X3A}"
  $X3 --input $WAV --output $TEMP_X3A

  echo "Decoding ${TEMP_X3A} to ${TEMP_WAV}"
  $X3 --input $TEMP_X3A --output $TEMP_WAV

  echo

done

