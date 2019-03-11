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

use crate::bitpack::BitReader;
use crate::crc::crc16;
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
fn peek_first_frame(br: &mut BitReader) -> Result<usize, X3Error> {
  let (payload_samples, payload_size) = get_frame_details(br)?;
  let remaining_bytes = br.remaining_bytes()?;

  // We are only peeking, let's reset the br.counter to the beginning of the frame
  br.dec_counter_n_bytes(x3::FrameHeader::LENGTH)?;

  // Wild estimate of the total size of the buffer,  reduces the need to make many memory allocs
  let estimated_length = estimate_total_length(payload_samples, payload_size, remaining_bytes);

  Ok(estimated_length)
}

fn find_next_frame(br: &mut BitReader) -> Result<(), X3Error> {
  br.inc_counter_n_bytes(1)?;
  br.find_u16_from_bookmark(x3::FrameHeader::KEY, br.remaining_bytes()?)?;
  Ok(())
}

///
/// Decode all available frames found in `br`.
///
/// ### Arguments
///
/// * `br` - the data to decode as a BitReader.
/// * `params` - the audio properties.
///
pub fn decode_frames(br: &mut BitReader, params: &x3::Parameters) -> Result<Vec<i16>, X3Error> {
  let estimated_length = peek_first_frame(br)?;

  // FIXME: Here we allocate memory in one huge array, there may be cases when this is not enough, and
  // in other cases it will be too much.
  let mut wav: Vec<i16> = vec![0; estimated_length];
  let mut p_wav = 0; // the pointer to keep a track of `wav` writes
  let mut total_samples_written = 0;
  while br.remaining_bytes()? > x3::FrameHeader::LENGTH {
    br.bookmark();
    match decode_frame(br, &mut wav, &params, &mut p_wav) {
      Ok(result) => total_samples_written += result,
      Err(_) => match find_next_frame(br) {
        Ok(()) => (),
        Err(_) => eprintln!("An error occurred decoding a frame"), // this is okay, since we only hit the end of the array
      },
    };
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
/// * `br` - the data to decode as a BitReader.
/// * `wav` - where the wav data will be written to.
/// * `params` - the audio properties.
/// * `p_wav` - the pointer to keep a track of `wav` writes.
///
pub fn decode_frame(
  br: &mut BitReader,
  wav: &mut Vec<i16>,
  params: &x3::Parameters,
  p_wav: &mut usize,
) -> Result<usize, X3Error> {
  if br.remaining_bytes()? < x3::FrameHeader::LENGTH {
    return Err(X3Error::FrameDecodeUnexpectedEnd);
  }

  // Frames are word aligned
  br.word_align();

  // Get the frame header
  br.reset_boundary();
  let (ns, payload_size) = get_frame_details(br)?;
  br.set_boundary(payload_size)?;

  #[cfg(not(feature = "oceaninstruments"))]
  let mut last_wav = br.read_be_i16()?;
  #[cfg(feature = "oceaninstruments")]
  let mut last_wav = br.read_le_i16()?;

  let mut remaining_samples = ns - 1;
  let mut samples_written = 0;

  while remaining_samples > 0 {
    let block_len = core::cmp::min(remaining_samples, params.block_len);

    decode_block(
      br,
      &mut wav[*p_wav..(*p_wav + params.block_len)],
      &mut last_wav,
      block_len,
      &params,
    )?;

    samples_written += block_len;
    remaining_samples -= block_len;
    *p_wav += block_len;
  }

  Ok(samples_written)
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
pub fn get_frame_details(br: &mut BitReader) -> Result<(usize, usize), X3Error> {
  // Calc header CRC
  let buf = &mut [0u8; x3::FrameHeader::HEADER_CRC_BYTE];
  br.peek_bytes(buf)?;
  let expected_header_crc = crc16(buf);

  // Read <Frame Key>
  if !br.eq(x3::FrameHeader::KEY_BUF)? {
    return Err(X3Error::FrameHeaderInvalidKey);
  }
  br.inc_counter_n_bytes(x3::FrameHeader::KEY_BUF.len())?;

  // <Source Id>
  // Currently just skip it
  br.inc_counter_n_bytes(1)?;

  // <Num Channels>
  let num_channels = br.read_nbits(8)?;
  if num_channels > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }

  // <Num Samples>
  let num_samples = br.read_be_u16()? as usize;

  // <Payload Length>
  let payload_len = br.read_be_u16()? as usize;
  if payload_len >= x3::Frame::MAX_LENGTH {
    return Err(X3Error::FrameLength);
  }

  // <Time>
  // Skip time
  br.inc_counter_n_bytes(8)?;

  // <Header CRC>
  let header_crc = br.read_be_u16()?;
  if expected_header_crc != header_crc {
    return Err(X3Error::FrameHeaderInvalidHeaderCRC);
  }

  // <Payload CRC>
  let payload_crc = br.read_be_u16()?;
  let mut payload = vec![0; payload_len];
  br.peek_bytes(&mut payload)?;
  let expected_payload_crc = crc16(&payload);
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
pub fn get_frame_details(br: &mut BitReader) -> Result<(usize, usize), X3Error> {
  // Calc header CRC
  let buf = &mut [0u8; x3::FrameHeader::HEADER_CRC_BYTE];
  br.peek_bytes(buf)?;
  // let expected_header_crc = crc16(buf);

  // Read <Frame Key>
  if !br.eq(x3::FrameHeader::KEY_BUF)? {
    return Err(X3Error::FrameHeaderInvalidKey);
  }
  br.inc_counter_n_bytes(x3::FrameHeader::KEY_BUF.len())?;

  // <Source Id>
  // Currently just skip it
  br.inc_counter_n_bytes(1)?;

  // <Num Channels>
  let num_channels = br.read_nbits(8)?;
  if num_channels > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }

  // <Payload Length>
  let payload_len = br.read_be_u16()? as usize;
  if payload_len >= x3::Frame::MAX_LENGTH {
    return Err(X3Error::FrameLength);
  }

  // <Num Samples>
  let num_samples = br.read_be_u16()? as usize;

  // <Time>
  // Skip time
  br.inc_counter_n_bytes(8)?;

  // Unused
  br.inc_counter_n_bytes(2)?;

  // Ignore sample rate
  let _sample_rate = br.inc_counter_n_bytes(2)?;

  // TODO: This may be turned on the future
  // <Header CRC>
  let _header_crc = br.read_be_u16()?;
  // if expected_header_crc != header_crc {
  //   return Err(X3Error::FrameHeaderInvalidHeaderCRC);
  // }

  // <Payload CRC>
  let _payload_crc = br.read_be_u16()?;
  let mut payload = vec![0; payload_len];
  br.peek_bytes(&mut payload)?;
  // let expected_payload_crc = crc16(&payload);
  // if expected_payload_crc != payload_crc {
  //   return Err(X3Error::FrameHeaderInvalidPayloadCRC);
  // }

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
  block_len: usize,
  params: &x3::Parameters,
) -> Result<(), X3Error> {
  let ftype = br.read_nbits(2)? as usize;
  if ftype != 0 {
    decode_ricecode_block(br, wav, last_wav, block_len, params, ftype)?;
  } else {
    decode_bpf_block(br, wav, last_wav, block_len)?;
  }

  for w in wav {
    print!("{} ", w);
  }
  println!();

  Ok(())
}

fn decode_ricecode_block(
  br: &mut BitReader,
  wav: &mut [i16],
  last_wav: &mut i16,
  block_len: usize,
  params: &x3::Parameters,
  ftype: usize,
) -> Result<(), X3Error> {
  let code = params.rice_codes[ftype - 1];
  if ftype == 1 {
    for wav_value in wav.iter_mut().take(block_len) {
      let n = br.read_zero_bits()?;
      br.read_nbits(1)?; // skip the next bit
      *last_wav += code.inv[n]; // Table lookup to convert to a signed number
      *wav_value = *last_wav;
    }
  } else if ftype == 2 || ftype == 3 {
    let nb = if ftype == 2 { 2 } else { 4 };
    let level = 1 << code.nsubs;
    for wav_value in wav.iter_mut().take(block_len) {
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
  Ok(())
}

fn decode_bpf_block(br: &mut BitReader, wav: &mut [i16], last_wav: &mut i16, block_len: usize) -> Result<(), X3Error> {
  // This is a BFP or pass-through block
  let num_bits = (br.read_nbits(4)? + 1) as usize; // Read the rest of the block header
  if num_bits == 16 {
    // This is a pass-through block
    for wav_value in wav.iter_mut().take(block_len) {
      *wav_value = br.read_nbits(16)? as i16;
    }
  } else {
    // Otherwise, this is a BFP-encoded block with E + 1 bits/word
    let neg_thresh = 1 << (num_bits - 1);
    let neg = 1 << num_bits;
    let mut value = i32::from(*last_wav);
    for wav_value in wav.iter_mut().take(block_len) {
      let mut diff = i32::from(br.read_nbits(num_bits)?);

      // Need to convert this to a signed integer
      if diff > neg_thresh {
        diff -= neg;
      }
      value += diff;
      *wav_value = value as i16;
    }
  }
  *last_wav = wav[wav.len() - 1];

  Ok(())
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
    for _ in 0..6 {
      br.inc_counter().unwrap();
    }
    decode_block(&mut br, wav, &mut last_wav, 19, params).unwrap();

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
    decode_block(&mut br, wav, &mut last_wav, 19, params).unwrap();

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
    decode_block(&mut br, wav, &mut last_wav, 19, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_bpf_eq16() {
    let x3_inp: &mut [u8] = &mut [
      129, 171, 62, 250, 4, 71, 75, 230, 252, 150, 153, 97, 24, 220, 83, 53, 143, 92, 101, 211, 155, 34, 73, 241, 221,
      200, 202, 252, 149, 240, 72, 20, 156, 172, 146, 59, 245, 23, 131, 33, 100, 0,
    ];
    let wav: &mut [i16] = &mut [0i16; 20];
    let expected_wavput = [
      -16767, 4562, -1601, 9638, 22598, 14100, -12957, -10471, 29926, -14190, 31863, 29234, -16603, 31762, 1319, 11044,
      -28931, 17888, -14247,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, 19, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

  #[test]
  fn test_decode_block_bpf_lt16() {
    let x3_inp: &mut [u8] = &mut [
      242, 73, 24, 151, 240, 252, 191, 163, 225, 164, 48, 158, 196, 188, 251, 246, 20, 31, 240, 96,
    ];
    let wav: &mut [i16] = &mut [0i16; 20];
    let expected_wavput = [
      -3493, -3494, -3487, -3501, -3502, -3467, -3483, -3506, -3500, -3491, -3501, -3483, -3490, -3495, -3500, -3495,
      -3492, -3493, -3490,
    ];

    let mut last_wav = BigEndian::read_i16(&x3_inp[0..2]);
    let mut br = BitReader::new(&mut x3_inp[2..]);
    let params = &x3::Parameters::default();
    decode_block(&mut br, wav, &mut last_wav, 19, params).unwrap();

    assert_eq!(expected_wavput, &mut wav[0..expected_wavput.len()]);
  }

}
