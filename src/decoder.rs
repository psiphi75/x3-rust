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

use crate::bitpack::{BitPackError, BitReader, ByteReader};
use crate::crc;
use crate::error;
use crate::x3;

use byteorder::{BigEndian, ByteOrder};
use error::X3Error;
// use std::io::BufReader;
use crate::x3::FrameHeader;

// Compression won't be more than 10x
fn estimate_total_length(num_bytes: usize) -> usize {
  num_bytes * 10
}

pub enum FrameTest {
  IsFrame,
  EndOfBuffer,
  NotFrame,
}

pub fn move_to_next_frame(bytes: &mut ByteReader) -> Result<(), X3Error> {
  loop {
    match is_valid_frame(bytes) {
      FrameTest::IsFrame => return Ok(()),
      FrameTest::EndOfBuffer => return Err(X3Error::FrameDecodeUnexpectedEnd),
      FrameTest::NotFrame => {
        bytes.inc_counter(1)?;
        bytes.find_le_u16(FrameHeader::KEY);
      }
    }
  }
}

///
/// Decode all available frames found in `bytes`.
///
/// ### Arguments
///
/// * `bytes` - the data to decode as a ByteReader.
/// * `params` - the audio properties.
///
/// ### Returns
///
/// * (bytes, errors) - The bytes read and the number of encoding errors encountered
///
pub fn decode_frames(bytes: &mut ByteReader, params: &x3::Parameters) -> Result<Option<(Vec<i16>, usize)>, X3Error> {
  let estimated_length = estimate_total_length(bytes.remaining_bytes()?);

  let mut wav: Vec<i16> = vec![0; estimated_length];
  let mut p_wav = 0; // the pointer to keep a track of `wav` writes
  let mut total_samples_written = 0;
  let mut errors = 0;
  while bytes.remaining_bytes()? > FrameHeader::LENGTH {
    let mut samples_written = 0;
    match decode_frame(bytes, &mut wav, &params, &mut p_wav, &mut samples_written) {
      Ok(()) => {}
      Err(X3Error::FrameHeaderInvalidPayloadLen) => eprintln!("The final frame was not complete"),
      Err(err) => {
        errors += 1;
        println!("ERROR occurred: {:?}", err);
        match move_to_next_frame(bytes) {
          Ok(()) => (),
          Err(_) => {
            // this is okay, since we only hit the end of the array
            errors += 1;
            eprintln!("An error occurred decoding a frame");
          }
        }
      }
    };
    total_samples_written += samples_written;
  }

  // Resize to the length of uncompressed bytes
  wav.resize(total_samples_written, 0);

  Ok(Some((wav, errors)))
}

// pub fn decode_frames_NEW(bytes: &mut X3aReader, params: &x3::Parameters) -> Result<Option<(Vec<i16>, usize)>, X3Error> {
//   let estimated_length = estimate_total_length(bytes.remaining_bytes()?);

//   let mut wav: Vec<i16> = vec![0; estimated_length];
//   let mut p_wav = 0; // the pointer to keep a track of `wav` writes
//   let mut total_samples_written = 0;
//   let mut errors = 0;
//   while bytes.remaining_bytes()? > FrameHeader::LENGTH {
//     let mut samples_written = 0;
//     match decode_frame(bytes, &mut wav, &params, &mut p_wav, &mut samples_written) {
//       Ok(()) => {}
//       Err(X3Error::FrameHeaderInvalidPayloadLen) => eprintln!("The final frame was not complete"),
//       Err(err) => {
//         errors += 1;
//         println!("ERROR occurred: {:?}", err);
//         match move_to_next_frame(bytes) {
//           Ok(()) => (),
//           Err(_) => {
//             // this is okay, since we only hit the end of the array
//             errors += 1;
//             eprintln!("An error occurred decoding a frame");
//           }
//         }
//       }
//     };
//     total_samples_written += samples_written;
//   }

//   // Resize to the length of uncompressed bytes
//   wav.resize(total_samples_written, 0);

//   Ok(Some((wav, errors)))
// }

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

  let mut last_wav = bytes.read_be_i16()?;

  wav[*p_wav] = last_wav as i16;
  *p_wav += 1;
  *samples_written += 1;

  let br_payload_size = payload_size - 2;
  let mut buf = &mut vec![0; br_payload_size];
  let bytes_written = bytes.read(&mut buf)?;
  let br = &mut BitReader::new(&mut buf[..bytes_written]);

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

pub fn decode_frame_NEW(
  bytes: &mut ByteReader,
  wav: &mut [i16],
  params: &x3::Parameters,
  p_wav: &mut usize,
  samples_written: &mut usize,
  samples: usize,
) -> Result<(), X3Error> {
  let mut last_wav = bytes.read_be_i16()?;

  wav[*p_wav] = last_wav as i16;
  *p_wav += 1;
  *samples_written += 1;

  let br_payload_len = bytes.remaining_bytes()?;
  let mut buf = &mut vec![0; br_payload_len];
  let bytes_written = bytes.read(&mut buf)?;
  let br = &mut BitReader::new(&mut buf[..bytes_written]);

  let mut remaining_samples = samples - 1;

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
/// Check if `bytes` are currently pointing to a valid frame.  It checks the
/// header and the CRC packets.  It also looks for the next frame.
///
pub fn is_valid_frame(bytes: &mut ByteReader) -> FrameTest {
  let p_byte = bytes.get_pos();
  let result = read_frame_header(bytes);
  bytes.set_pos(p_byte);

  match result {
    Ok(_) => FrameTest::IsFrame,
    Err(X3Error::BitPack(BitPackError::ArrayEndReached)) => FrameTest::EndOfBuffer,
    Err(X3Error::FrameDecodeUnexpectedEnd) => FrameTest::EndOfBuffer,
    Err(_) => FrameTest::NotFrame,
  }
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
pub fn read_frame_header_NEW(bytes: &[u8]) -> Result<FrameHeader, X3Error> {
  if bytes.len() < FrameHeader::LENGTH {
    return Err(X3Error::FrameDecodeUnexpectedEnd);
  }
  // Calc header CRC
  let header_crc = crc::crc16(&bytes[0..FrameHeader::P_HEADER_CRC]);
  let expected_header_crc = BigEndian::read_u16(&bytes[FrameHeader::P_HEADER_CRC..]);
  if expected_header_crc != header_crc {
    return Err(X3Error::FrameHeaderInvalidHeaderCRC);
  }

  // Read <Frame Key>
  let x3_archive_key = BigEndian::read_u16(&bytes[0..2]);
  if x3_archive_key != FrameHeader::KEY {
    return Err(X3Error::FrameHeaderInvalidKey);
  }

  // <Source Id>
  // Currently just skip it
  let source_id = bytes[FrameHeader::P_SOURCE_ID];

  // <Num Channels>
  let channels = bytes[FrameHeader::P_CHANNELS];
  if channels > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }

  // <Num Samples>
  let samples = BigEndian::read_u16(&bytes[FrameHeader::P_SAMPLES..]);

  // <Payload Length>
  let payload_len = BigEndian::read_u16(&bytes[FrameHeader::P_PAYLOAD_SIZE..]) as usize;
  if payload_len >= x3::Frame::MAX_LENGTH {
    return Err(X3Error::FrameLength);
  }

  // <Time>
  // Skip time

  // <Payload CRC>
  let payload_crc = BigEndian::read_u16(&bytes[FrameHeader::P_PAYLOAD_CRC..]);

  Ok(FrameHeader {
    source_id,
    samples,
    channels,
    payload_len,
    payload_crc,
  })
}

pub fn read_frame_header(bytes: &mut ByteReader) -> Result<(usize, usize), X3Error> {
  if bytes.remaining_bytes()? < x3::FrameHeader::LENGTH {
    return Err(X3Error::FrameDecodeUnexpectedEnd);
  }

  // Calc header CRC
  let expected_header_crc = bytes.crc16(x3::FrameHeader::P_HEADER_CRC)?;

  // Read <Frame Key>
  if !bytes.eq(x3::FrameHeader::KEY_BUF) {
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
