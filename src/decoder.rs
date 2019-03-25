/**************************************************************************
 *                                                                        *
 * Rust implementation of the X3 lossless audio compression protocol.     *
 *                                                                        *
 * Copyright (C) 2019 Simon M. Werner <simonwerner@gmail.com>             *
 *                                                                        *
 * This program is free software; you can redistribute it and/or modify   *
 * it under the terms of the GNU General Public License as published by   *
 * the Free Software Foundation, either version 3 of the License, or      *
 * (at your option) any later version.                                    *
 *                                                                        *
 * This program is distributed in the hope that it will be useful,        *
 * but WITHOUT ANY WARRANTY; without even the implied warranty of         *
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the           *
 * GNU General Public License for more details.                           *
 *                                                                        *
 * You should have received a copy of the GNU General Public License      *
 * along with this program. If not, see <http://www.gnu.org/licenses/>.   *
 *                                                                        *
 **************************************************************************/

/*
 *    This work contains a derivative of the work from John Atkins and Mark Johnson.
 *    This license applies to the following functions. `unpackr`, `integrate`, `unpack`,
 *    and `decode_block` (oceaninstruments) functions.
 *
 *    Copyright (C) 2011-2014, John Atkins and Mark Johnson
 *
 *    This work is a derivative of the D3-API Copyright (C) 2008-2010, Mark Johnson
 *
 *    This file is part of the SoundTrap software. SoundTrap is an acoustic recording
 *    system intended for underwater acoustic measurements. This component of the
 *    SoundTrap project is free software: you can redistribute it and/or modify it
 *    under the terms of the GNU General Public License as published by the Free Software
 *    Foundation, either version 3 of the License, or any later version.
 *
 *    The SoundTrap software is distributed in the hope that it will be useful, but
 *    WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 *    FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
 *
 *    You should have received a copy of the GNU General Public License along with this
 *    code. If not, see <http://www.gnu.org/licenses/>.
 */

use crate::bitpack::{BitReader, ByteReader};
use crate::error;
use crate::x3;

use error::X3Error;

// Bad estimator for the total expected length of the output wav file.
fn estimate_total_length(num_samples: usize, compressed_len: usize, remaining_bytes: usize) -> usize {
  const ESTIMATION_FACTOR: f32 = 1.5;
  const WORD_SIZE: f32 = (x3::Parameters::WAV_BIT_SIZE / 8) as f32;
  let compression_ratio = num_samples as f32 / compressed_len as f32;

  (compression_ratio * remaining_bytes as f32 * ESTIMATION_FACTOR * WORD_SIZE).round() as usize
}

// Look at the first frame in a sequence of frames, this is just so we can allocate enough memory to
// process everything.
fn peek_first_frame(bytes: &mut ByteReader) -> Result<usize, X3Error> {
  let (payload_samples, payload_size) = read_frame_header(bytes)?;
  let remaining_bytes = bytes.remaining_bytes()?;

  // We are only peeking, let's reset the br.counter to the beginning of the frame
  bytes.dec_counter(x3::FrameHeader::LENGTH)?;

  // Wild estimate of the total size of the buffer,  reduces the need to make many memory allocs
  let estimated_length = estimate_total_length(payload_samples, payload_size, remaining_bytes);

  Ok(estimated_length)
}

fn find_next_frame(bytes: &mut ByteReader) -> Result<(), X3Error> {
  bytes.inc_counter(1)?;
  Ok(())
}

///
/// Decode all available frames found in `bytes`.
///
/// ### Arguments
///
/// * `bytes` - the data to decode as a ByteReader.
/// * `params` - the audio properties.
///
pub fn decode_frames(bytes: &mut ByteReader, params: &x3::Parameters) -> Result<Vec<i16>, X3Error> {
  let estimated_length = peek_first_frame(bytes)?;

  // FIXME: Here we allocate memory in one huge array, there may be cases when this is not enough, and
  // in other cases it will be too much.
  let mut wav: Vec<i16> = vec![0; estimated_length];
  let mut p_wav = 0; // the pointer to keep a track of `wav` writes
  let mut total_samples_written = 0;
  while bytes.remaining_bytes()? > x3::FrameHeader::LENGTH {
    let mut samples_written = 0;
    match decode_frame(bytes, &mut wav, &params, &mut p_wav, &mut samples_written) {
      Ok(()) => {}
      Err(X3Error::FrameHeaderInvalidPayloadLen) => eprintln!("The final frame was not complete"),
      Err(_) => match find_next_frame(bytes) {
        Ok(()) => (),
        Err(_) => eprintln!("An error occurred decoding a frame"), // this is okay, since we only hit the end of the array
      },
    };
    total_samples_written += samples_written;
  }

  // Resize to the length of uncompressed bytes
  wav.resize(total_samples_written, 0);

  Ok(wav)
}

///
/// Decode an individual frame.
///
/// ### Arguments
///
/// * `br` - the data to decode as a ByteReader.
/// * `wav` - where the wav data will be written to.
/// * `params` - the audio properties.
/// * `p_wav` - the pointer to keep a track of `wav` writes.
///
pub fn decode_frame(
  bytes: &mut ByteReader,
  wav: &mut Vec<i16>,
  params: &x3::Parameters,
  p_wav: &mut usize,
  samples_written: &mut usize,
) -> Result<(), X3Error> {
  // Get the frame header
  let (ns, payload_size) = read_frame_header(bytes)?;

  #[cfg(not(feature = "oceaninstruments"))]
  let mut last_wav = bytes.read_be_i16()?;
  #[cfg(feature = "oceaninstruments")]
  let mut last_wav = i32::from(bytes.read_le_i16()?);

  wav[*p_wav] = last_wav as i16;
  *p_wav += 1;
  *samples_written += 1;

  let br_payload_size = payload_size - 2;
  let mut buf = &mut vec![0; br_payload_size];
  bytes.read(&mut buf)?;
  let br = &mut BitReader::new(&mut buf);

  let mut remaining_samples = ns - 1;

  while remaining_samples > 0 {
    let block_len = core::cmp::min(remaining_samples, params.block_len);
    let block_len = decode_block(br, &mut wav[*p_wav..(*p_wav + block_len)], &mut last_wav, &params)?;

    *samples_written += block_len;
    remaining_samples -= block_len;
    *p_wav += block_len;
  }

  Ok(())
}

///
/// Parse the frame header and return the payload.  The Frame header and payload
/// contain CRCs, theses will be checked and errors returned if the CRC does not
/// match.
///
/// ### Arguments
///
/// * `br` - the data to decode as a BitReader.
///
#[cfg(not(feature = "oceaninstruments"))]
pub fn read_frame_header(bytes: &mut ByteReader) -> Result<(usize, usize), X3Error> {
  if bytes.remaining_bytes()? < x3::FrameHeader::LENGTH {
    return Err(X3Error::FrameDecodeUnexpectedEnd);
  }

  // Calc header CRC
  let expected_header_crc = bytes.crc16(x3::FrameHeader::HEADER_CRC_BYTE)?;

  // Read <Frame Key>
  if !bytes.eq(x3::FrameHeader::KEY_BUF)? {
    return Err(X3Error::FrameHeaderInvalidKey);
  }
  bytes.inc_counter(x3::FrameHeader::KEY_BUF.len())?;

  // <Source Id>
  // Currently just skip it
  bytes.inc_counter(1)?;

  // <Num Channels>
  let num_channels = bytes.read_u8()?;
  if num_channels > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }

  // <Num Samples>
  let num_samples = bytes.read_be_u16()? as usize;

  // <Payload Length>
  let payload_len = bytes.read_be_u16()? as usize;
  if payload_len >= x3::Frame::MAX_LENGTH {
    return Err(X3Error::FrameLength);
  }

  // <Time>
  // Skip time
  bytes.inc_counter(8)?;

  // <Header CRC>
  let header_crc = bytes.read_be_u16()?;
  if expected_header_crc != header_crc {
    return Err(X3Error::FrameHeaderInvalidHeaderCRC);
  }

  // <Payload CRC>
  let payload_crc = bytes.read_be_u16()?;
  if bytes.remaining_bytes()? < payload_len {
    return Err(X3Error::FrameHeaderInvalidPayloadLen);
  }
  let expected_payload_crc = bytes.crc16(payload_len)?;
  if expected_payload_crc != payload_crc {
    return Err(X3Error::FrameHeaderInvalidPayloadCRC);
  }

  Ok((num_samples, payload_len))
}

// Uses the following setup for the FrameHeader:
//   Preamble for synchronisation (2 bytes) - "ST"
//   Packet Type (2 bytes) - ignored
//   Packet size (2 bytes) - Big Endian
//   *Number of samples (2 bytes) - Big Endian
//   Acquisition Date/Time Stamp (unix time) (4 bytes) - Big Endian
//   Acquisition Date/Time Stamp (microseconds) (4 bytes) - Big Endian
//   Unused (2 bytes) - ignored
//   *SampleRate(2 bytes) - Big Endian
//   Unused (4 bytes) - ignored
//   Audio Data (up to 2048 bytes)
#[cfg(feature = "oceaninstruments")]
pub fn read_frame_header(bytes: &mut ByteReader) -> Result<(usize, usize), X3Error> {
  if bytes.remaining_bytes()? < x3::FrameHeader::LENGTH {
    return Err(X3Error::FrameDecodeUnexpectedEnd);
  }

  // Calc header CRC
  let _expected_header_crc = bytes.crc16(x3::FrameHeader::HEADER_CRC_BYTE)?;

  // Read <Frame Key>
  if !bytes.eq(x3::FrameHeader::KEY_BUF)? {
    return Err(X3Error::FrameHeaderInvalidKey);
  }
  bytes.inc_counter(x3::FrameHeader::KEY_BUF.len())?;

  // <Source Id>
  // Currently just skip it
  bytes.inc_counter(1)?;

  // <Num Channels>
  let num_channels = bytes.read_u8()?;
  if num_channels > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }

  // <Payload Length> - length in bytes, it's stored as length in words
  let payload_len = bytes.read_be_u16()? as usize * 2;
  if payload_len >= x3::Frame::MAX_LENGTH {
    return Err(X3Error::FrameLength);
  }

  // <Num Samples>
  let num_samples = bytes.read_be_u16()? as usize;

  // <Time>
  // Skip time
  bytes.inc_counter(8)?;

  // Unused
  bytes.inc_counter(2)?;

  // Ignore sample rate
  let _sample_rate = bytes.inc_counter(2)?;

  // TODO: This may be turned on the future
  // <Header CRC>
  let _header_crc = bytes.read_be_u16()?;
  // if expected_header_crc != header_crc {
  //   return Err(X3Error::FrameHeaderInvalidHeaderCRC);
  // }

  // <Payload CRC>
  let _payload_crc = bytes.read_be_u16()?;
  if bytes.remaining_bytes()? < payload_len {
    return Err(X3Error::FrameHeaderInvalidPayloadLen);
  }
  let _expected_payload_crc = bytes.crc16(payload_len)?;
  // if expected_payload_crc != payload_crc {
  //   return Err(X3Error::FrameHeaderInvalidPayloadCRC);
  // }

  Ok((num_samples, payload_len))
}

pub fn move_to_next_frame(bytes: &mut ByteReader) {
  bytes.find_le_u16(x3::FrameHeader::KEY);
}

///
/// Decode a block of compressed x3 data.  This function will determine weather to
/// use the Rice Code method, or the BFP method.
///
/// ### Arguments
///
/// * `br` - the data to decode as a BitReader.
/// * `wav` - where the wav data will be written to.
/// * `last_wav` - the last value of the previous block.
/// * `block_len` - how many bytes the decoded block will be.
/// * `params` - the audio properties.
///
#[cfg(not(feature = "oceaninstruments"))]
pub fn decode_block(
  br: &mut BitReader,
  wav: &mut [i16],
  last_wav: &mut i16,
  params: &x3::Parameters,
) -> Result<usize, X3Error> {
  let ftype = br.read_nbits(2)? as usize;
  if ftype == 0 {
    decode_bpf_block(br, wav, last_wav)
  } else {
    decode_ricecode_block(br, wav, last_wav, params, ftype)
  }
}

#[cfg(not(feature = "oceaninstruments"))]
fn decode_ricecode_block(
  br: &mut BitReader,
  wav: &mut [i16],
  last_wav: &mut i16,
  params: &x3::Parameters,
  ftype: usize,
) -> Result<usize, X3Error> {
  let code = params.rice_codes[ftype - 1];
  if ftype == 1 {
    for wav_value in wav.iter_mut() {
      let n = br.read_zero_bits()?;
      br.read_nbits(1)?; // skip the next bit
      *last_wav += code.inv[n]; // Table lookup to convert to a signed number
      *wav_value = *last_wav;
    }
  } else if ftype == 2 || ftype == 3 {
    let nb = if ftype == 2 { 2 } else { 4 };
    let level = 1 << code.nsubs;
    for wav_value in wav.iter_mut() {
      let n = br.read_zero_bits()? as i16;
      let r = br.read_nbits(nb)? as i16;
      let i = r + level * (n - 1);
      if i as usize >= code.inv_len {
        return Err(X3Error::OutOfBoundsInverse);
      }
      let diff = code.inv[i as usize];
      *last_wav += diff;
      *wav_value = *last_wav;
    }
  }
  Ok(wav.len())
}

fn unsigned_to_i16(a: u16, num_bits: usize) -> i16 {
  let mut a = i32::from(a);
  let neg_thresh = 1 << (num_bits - 1);
  let neg = 1 << num_bits;
  // Need to convert this to a signed integer
  if a > neg_thresh {
    a -= neg;
  }
  a as i16
}

#[cfg(not(feature = "oceaninstruments"))]
fn decode_bpf_block(br: &mut BitReader, wav: &mut [i16], last_wav: &mut i16) -> Result<usize, X3Error> {
  // This is a BFP or pass-through block
  let num_bits = (br.read_nbits(4)? + 1) as usize; // Read the rest of the block header

  if num_bits <= 5 {
    // We can't have BPF with length 5 or less.
    return Err(X3Error::FrameDecodeInvalidBPF);
  }

  if num_bits == 16 {
    // This is a pass-through block
    for wav_value in wav.iter_mut() {
      *wav_value = br.read_nbits(16)? as i16;
    }
  } else {
    // Otherwise, this is a BFP-encoded block with E + 1 bits/word
    let mut value = *last_wav;
    for wav_value in wav.iter_mut() {
      let diff = br.read_nbits(num_bits)?;
      value += unsigned_to_i16(diff, num_bits);
      *wav_value = value as i16;
    }
  }
  *last_wav = wav[wav.len() - 1];

  Ok(wav.len())
}

#[cfg(feature = "oceaninstruments")]
const RSUFFS: [usize; 3] = [0, 1, 3];
#[cfg(feature = "oceaninstruments")]
const IRT: &'static [i16; 60] = &[
  0, -1, 1, -2, 2, -3, 3, -4, 4, -5, 5, -6, 6, -7, 7, -8, 8, -9, 9, -10, 10, -11, 11, -12, 12, -13, 13, -14, 14, -15,
  15, -16, 16, -17, 17, -18, 18, -19, 19, -20, 20, -21, 21, -22, 22, -23, 23, -24, 24, -25, 25, -26, 26, -27, 27, -28,
  28, -29, 29, -30,
];

#[cfg(feature = "oceaninstruments")]
fn unpackr(br: &mut BitReader, wav: &mut [i16], n: usize, code: usize) -> Result<(), X3Error> {
  // unpacker for variable length Rice codes
  // Returns 0 if ok, 1 if there are not enough bits in the stream.
  let mut ow = 0;
  let mut msk;
  let mut ntogo = 0;
  let mut ns;
  let mut suff;
  let nsuffix = RSUFFS[code];
  let lev = 1 << nsuffix;

  for k in 0..n {
    // Do for n words
    // First find the end of the variable length section.
    // If there is an end and a complete suffix in the current word, it will
    // have a value of at least 1<<nsuffix. If not, append the next word from
    // the stream

    ntogo = br.read_int_larger_than(lev, &mut ow)?;

    // ow is now guaranteed to have a start and a suffix.
    // Find the start (i.e., the first 1 bit from the left)
    ns = 1;
    msk = 1 << (ntogo - 1);
    while ns <= ntogo && (ow & msk) == 0 {
      ns += 1;
      msk >>= 1;
    }
    if ns > ntogo {
      return Err(X3Error::FrameDecodeInvalidRiceCode); //error
    }
    ntogo -= ns + nsuffix;
    suff = (ow >> ntogo) & (lev - 1);
    ow &= (1 << ntogo) - 1;
    let idx = lev * (ns - 1) + suff;
    if idx >= IRT.len() {
      return Err(X3Error::FrameDecodeInvalidIndex);
    }
    wav[k] = IRT[idx];
  }
  if ntogo > 0 {
    return Err(X3Error::FrameDecodeInvalidNTOGO); //error
  }

  Ok(())
}

#[cfg(feature = "oceaninstruments")]
fn integrate(wav: &mut [i16], last_wav: &mut i32, count: usize) {
  // De-emphasis filter to reverse the diff in the compressor.
  // Filter operates in-place.
  for k in 0..count {
    *last_wav += i32::from(wav[k]);
    wav[k] = *last_wav as i16;
  }
}

#[cfg(feature = "oceaninstruments")]
fn unpack(br: &mut BitReader, wav: &mut [i16], nb: usize, count: usize) -> Result<(), X3Error> {
  for i in 0..count {
    wav[i] = unsigned_to_i16(br.read_nbits(nb)?, nb);
  }
  Ok(())
}

#[cfg(feature = "oceaninstruments")]
pub fn decode_block(
  br: &mut BitReader,
  wav: &mut [i16],
  last_wav: &mut i32,
  _params: &x3::Parameters,
) -> Result<usize, X3Error> {
  let mut nb = 0;
  let mut code = br.read_nbits(2)?;
  let mut count = wav.len();
  if code == 0 {
    // bfp or pass thru block
    nb = br.read_nbits(4)?;
    if nb > 0 {
      nb += 1;
    } else {
      let nn = (br.read_nbits(6)? + 1) as usize;
      if nn > wav.len() {
        return Err(X3Error::FrameDecodeInvalidBlockLength);
      }
      count = nn;
      code = br.read_nbits(2)?;
      if code == 0 {
        nb = br.read_nbits(4)? + 1;
      }
    }
  }
  if code > 0 {
    unpackr(br, wav, count as usize, (code - 1) as usize)?;
  } else {
    unpack(br, wav, nb as usize, count)?;
    if nb == 16 {
      return Ok(count);
    }
  }

  integrate(wav, last_wav, wav.len());

  Ok(count)
}

//
//
//            #######
//               #       ######     ####     #####     ####
//               #       #         #           #      #
//               #       #####      ####       #       ####
//               #       #              #      #           #
//               #       #         #    #      #      #    #
//               #       ######     ####       #       ####
//
//

#[cfg(not(feature = "oceaninstruments"))]
#[cfg(test)]
mod tests {
  use crate::bitpack::BitReader;
  use crate::byteorder::{BigEndian, ByteOrder};
  use crate::decoder::decode_block;
  use crate::x3;

  #[test]
  fn test_decode_block_ftype_1() {
    let x3_inp: &mut [u8] = &mut [
      0x01, 0x10, 0x23, 0x18, 0x14, 0x90, 0x40, 0x82, 0x58, 0x41, 0x02, 0x0C, 0x4C,
    ];
    let wav: &mut [i16] = &mut [0i16; 20];
    let expected_wavput = [
      -375, -372, -374, -374, -376, -376, -373, -374, -373, -372, -375, -372, -375, -374, -375, -375, -373, -376, -373,
    ];

    let mut last_wav = -373;
    let mut br = BitReader::new(x3_inp);
    let params = &x3::Parameters::default();

    // Skip 6 bits
    br.read_nbits(6).unwrap();

    decode_block(&mut br, wav, &mut last_wav, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_ftype_2() {
    let x3_inp: &mut [u8] = &mut [
      0xf2, 0x76, 0xb1, 0x82, 0x14, 0xd0, 0x4, 0x4, 0x58, 0x18, 0x30, 0x20, 0x69, 0x86, 0x4, 0xfc, 0xc2, 0xf8, 0xaa,
      0x7f, 0xa1, 0xa, 0xfa, 0xad, 0xbc, 0x9d, 0x8d, 0x13, 0xc9, 0x66, 0xea, 0x5, 0xa3, 0x63, 0x94, 0xc9, 0xf4, 0x88,
      0x4e, 0xb3, 0x6, 0xc9, 0xdb, 0x8f, 0x70, 0x80, 0xb3, 0x8b, 0x6b, 0x14, 0x88, 0x5f, 0x6c, 0x2f, 0xaa, 0x5a, 0xae,
      0xf4, 0x29, 0x46, 0xd9, 0x12, 0x43, 0x4b, 0x4f, 0xd6, 0xeb, 0x24, 0xa8, 0x48, 0xc6, 0x3d, 0x1a, 0xb8, 0x71, 0x72,
      0xb5, 0x68, 0xb4, 0x5b, 0xa1, 0x7c, 0xb2, 0x48, 0x5f, 0x67, 0xd9, 0x1b, 0x65, 0x0,
    ];
    let wav: &mut [i16] = &mut [0i16; 20];
    let expected_wavput = [
      -3467, -3471, -3466, -3463, -3463, -3465, -3464, -3456, -3450, -3448, -3449, -3456, -3462, -3456, -3462, -3461,
      -3463, -3468, -3462,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_ftype_3() {
    let x3_inp: &mut [u8] = &mut [242, 123, 202, 56, 106, 202, 124, 8, 122, 249, 136, 173, 202, 23, 80];
    let wav: &mut [i16] = &mut [0i16; 20];
    let expected_wavput = [
      -3452, -3441, -3456, -3462, -3453, -3461, -3461, -3449, -3457, -3463, -3460, -3454, -3450, -3449, -3452, -3450,
      -3449, -3463, -3462,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_bpf_eq16() {
    let x3_inp: &mut [u8] = &mut [
      129, 171, 62, 250, 4, 71, 75, 230, 252, 150, 153, 97, 24, 220, 83, 53, 143, 92, 101, 211, 155, 34, 73, 241, 221,
      200, 202, 252, 149, 240, 72, 20, 156, 172, 146, 59, 245, 23, 131, 33, 100, 0,
    ];
    let wav: &mut [i16] = &mut [0i16; 19];
    let expected_wavput = [
      -16767, 4562, -1601, 9638, 22598, 14100, -12957, -10471, 29926, -14190, 31863, 29234, -16603, 31762, 1319, 11044,
      -28931, 17888, -14247,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_bpf_lt16() {
    let x3_inp: &mut [u8] = &mut [
      242, 73, 24, 151, 240, 252, 191, 163, 225, 164, 48, 158, 196, 188, 251, 246, 20, 31, 240, 96,
    ];
    let wav: &mut [i16] = &mut [0i16; 19];
    let expected_wavput = [
      -3493, -3494, -3487, -3501, -3502, -3467, -3483, -3506, -3500, -3491, -3501, -3483, -3490, -3495, -3500, -3495,
      -3492, -3493, -3490,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

}
