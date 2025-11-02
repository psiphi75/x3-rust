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

// externs
use crate::byteorder::{BigEndian, ByteOrder};

// This crate
use crate::bitpacker::BitPacker;
use crate::crc::crc16;
use crate::error;
use crate::x3;

use error::X3Error;

#[cfg(feature = "std")]
use std::println;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::vec::Vec;

///
/// Encode a wav file (represented as `Channels`).  The output will be written to `bp`.
///
/// ### Arguments
///
/// * `channels` - The list of channels to encode.  // FIXME: This is currently only one.
/// * `bp` - A `BitPacker` where the compressed data will be written to.
///
pub fn encode<'a, I>(channels: &mut [&mut x3::IterChannel<I>], bp: &mut BitPacker) -> Result<(), X3Error>
where
  I: Iterator<Item = i16>,
{
  if channels.len() > 1 {
    return Err(X3Error::MoreThanOneChannel);
  }
  let ch = &mut channels[0];
  let wav = &mut ch.wav;

  let samples_per_frame = ch.params.block_len * ch.params.blocks_per_frame;

  let stats: &mut [usize; 6] = &mut [0; 6];

  #[cfg(any(feature = "alloc", feature = "std"))]
  {
    loop {
      let frame_buffer = wav.by_ref().take(samples_per_frame).collect::<Vec<i16>>();
      if frame_buffer.len() == 0 {
        break;
      }
      encode_frame(&frame_buffer, bp, &ch.params, stats)?;
    }
  }
  #[cfg(not(feature = "std"))]
  {
    // FIXME: This could still be more memory efficient by collecting this iterator on the block level instead of the frame level.
    // FIXME: This is the default frame size is used instead of maximum frame size
    let mut frame_buffer = [0i16; x3::Parameters::MAX_BLOCK_LENGTH * x3::Parameters::DEFAULT_BLOCKS_PER_FRAME];
    loop {
      // collect frame samples
      let mut frame_length = 0;
      for (i, fs) in wav.by_ref().take(samples_per_frame).enumerate() {
        frame_buffer[i] = fs;
        frame_length = i;
      }

      if frame_length == 0 {
        break;
      }

      encode_frame(&frame_buffer[..frame_length + 1], bp, &ch.params, stats)?;
    }
  }

  #[cfg(feature = "std")]
  {
    let t = (stats[0] + stats[1] + stats[2] + stats[3] + stats[4] + stats[5]) as f32;
    println!(
      "\nStatistics:\n  Rice-0: {:.4}%\n  Rice-1: {:.4}%\n  Rice-2: {:.4}%\n  Rice-3: {:.4}%\n  BFP: {:.4}%\n  Pass-through {:.4}%\n",
      (stats[0] as f32 / t) * 100.0,
      (stats[1] as f32 / t) * 100.0,
      (stats[2] as f32 / t) * 100.0,
      (stats[3] as f32 / t) * 100.0,
      (stats[4] as f32 / t) * 100.0,
      (stats[5] as f32 / t) * 100.0
    );
  }

  Ok(())
}

///
/// Write the frame header to the bit array.
///
/// ### Arguments
///
/// * `bp` - A `BitPacker` where the frame data will be written to.
/// * `num_samples` - The number of samples that are contained in the wav.
/// * `id` -  The source id.
///
pub fn write_frame_header(bp: &mut BitPacker, num_samples: usize, id: u8) -> Result<(), X3Error> {
  let header: &mut [u8; x3::FrameHeader::LENGTH] = &mut [0u8; x3::FrameHeader::LENGTH];

  // frame_len = header.len + payload.len
  let frame_len = bp.bookmark_get_offset();
  let payload_len = frame_len - x3::FrameHeader::LENGTH;

  // <Frame Key> = "x3"
  let mut p = 0;
  BigEndian::write_u16(&mut header[p..], x3::FrameHeader::KEY);
  p += 2;

  // <Source Id> = The source Id
  header[p] = id;
  p += 1;

  // <Num Channels> = The number of channels
  header[p] = id;
  p += 1;
  // FIXME: Should write the `channel.id` value
  // BigEndian::write_u8(&mut header[p..], channel.id);

  // <Num Samples> = The number of uncompressed samples.
  BigEndian::write_u16(&mut header[p..], num_samples as u16);
  p += 2;

  // <Payload Length> = The number of compressed bytes.
  BigEndian::write_u16(&mut header[p..], payload_len as u16);
  p += 2;

  // <Time> = The timestamp of the first sample in the frame.
  // FIXME: Need to add the time
  p += 8;

  // <Header CRC> = CRC of the frame header
  let header_crc = crc16(&header[0..x3::FrameHeader::P_HEADER_CRC]);
  BigEndian::write_u16(&mut header[p..], header_crc as u16);
  p += 2;

  // <Payload CRC> = CRC of the payload
  let frame = bp.bookmark_get_from();
  let payload_len = frame_len - x3::FrameHeader::LENGTH;
  let payload = &frame[x3::FrameHeader::LENGTH..(x3::FrameHeader::LENGTH + payload_len)];
  let payload_crc = crc16(payload);
  BigEndian::write_u16(&mut header[p..], payload_crc as u16);

  // Write it back to the bit stream
  bp.word_align();
  bp.bookmark_write(header);

  Ok(())
}

///
/// Encode one frame and all it's blocks.  This takes the wav and converts it x3 compressed
/// audio.
///
/// ### Arguments
/// * `wav` - the raw audio data.
/// * `last_wav` - the last wav value from the previous frame.
/// * `bp` - Where the output x3 compressed bits will be written to.
/// * `params` - The audio parameters.
/// * `stats` - Used for statistics which get printed out at the end.
///
pub fn encode_frame(
  wav: &[i16],
  bp: &mut BitPacker,
  params: &x3::Parameters,
  stats: &mut [usize; 6],
) -> Result<(), X3Error> {
  // Bookmark this location such that we can write the header here
  bp.bookmark();
  bp.inc_counter_n_bytes(x3::FrameHeader::LENGTH)?;

  // Write first sample, <Audio State>, as a raw value
  bp.write_bits(wav[0] as usize, 16);

  // This techincally has data shared across blocks, so use here instead
  let mut wav_diff = diff(wav);

  let blocks = wav[1..].chunks(params.block_len);
  for block in blocks {
    // pack the data block for each channel
    let ftype = x3_encode_block(block, &mut wav_diff, bp, params)?;
    stats[ftype] += block.len();
  }

  // Wrap the bit to the next significant bit
  bp.word_align();

  // Write the header details
  write_frame_header(bp, wav.len(), 1)?;

  Ok(())
}

//
// Calcuate the diff (first order differential) of the raw audio data.
//
// ### Return
// * the maximum absolute value found in the diff.
//
#[inline(always)]
fn diff<'a>(inp: &'a [i16]) -> impl Iterator<Item = i32> + 'a {
  inp.windows(2).map(|w| i32::from(w[1]) - i32::from(w[0])) // collect on block level
}

/// Count the number of bits that it takes to represent a number.
#[inline(always)]
fn count_bits(n: u32) -> u32 {
  32 - n.leading_zeros()
}

fn encode_rice_block(
  wav_diff: &[i32],
  bp: &mut BitPacker,
  params: &x3::Parameters,
  max_abs_inp_filtd: i32,
) -> Result<usize, X3Error> {
  // Use Rice encoding method

  let mut ftype: usize = 0; // find which code to use

  for t in &params.thresholds {
    if max_abs_inp_filtd > *t as i32 {
      ftype += 1;
    }
  }

  // 2 bit rice block header
  bp.write_bits(ftype as usize + 1, 2);
  let rc = params.rice_codes[ftype];
  let codes = rc.code;
  let num_bits = rc.num_bits;
  let offset = rc.offset;

  for w in wav_diff {
    let ii = (*w + offset as i32) as usize;
    let code = codes[ii];
    let rc_num_bits = num_bits[ii];
    let num_zeros = rc_num_bits - count_bits(code as u32) as usize;

    bp.write_packed_zeros(num_zeros);
    bp.write_bits(code, rc_num_bits - num_zeros);
  }

  Ok(rc.nsubs)
}

fn encode_bfp_block(wav_diff: &[i32], bp: &mut BitPacker, num_bits: usize) -> Result<usize, X3Error> {
  bp.write_bits(num_bits as usize, BFP_HDR_LEN);
  // Reduce the number of bits only.
  for wd in wav_diff {
    bp.write_bits(*wd as usize, num_bits as usize + 1);
  }
  Ok(4)
}

fn encode_literal(wav: &[i16], bp: &mut BitPacker) -> Result<usize, X3Error> {
  // We write all the bytes out without any compression
  bp.write_bits(15, BFP_HDR_LEN);
  for w in wav {
    bp.write_bits(*w as usize, i16::BITS as usize);
  }
  Ok(5)
}

/// This will encode NSAMPLES of data.
const BFP_HDR_LEN: usize = 6;
fn x3_encode_block(
  wav: &[i16],
  wav_diff_iter: &mut impl Iterator<Item = i32>,
  bp: &mut BitPacker,
  params: &x3::Parameters,
) -> Result<usize, X3Error> {
  //collect wav_diff
  let wav_diff: &mut [i32] = &mut [0i32; x3::Parameters::MAX_BLOCK_LENGTH];
  let mut max_abs_inp_filtd = 0;
  for (i, wd) in wav_diff_iter.take(wav.len()).enumerate() {
    wav_diff[i] = wd;
    max_abs_inp_filtd = max_abs_inp_filtd.max(wd.abs());
  }
  let wav_diff = &wav_diff[..wav.len()];

  if max_abs_inp_filtd <= params.thresholds[2] as i32 {
    // 2 bit rice block header
    encode_rice_block(wav_diff, bp, params, max_abs_inp_filtd)
  } else {
    let num_bits = count_bits(max_abs_inp_filtd as u32) as usize; // number of bits
    if num_bits >= 15 {
      encode_literal(wav, bp)
    } else {
      encode_bfp_block(wav_diff, bp, num_bits)
    }
  }
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

  use crate::bitpacker::BitPacker;
  use crate::encoder::{self, diff};
  use crate::encoder::{encode_frame, x3_encode_block};
  use crate::x3;
  use crate::x3::Parameters;

  extern crate std;
  use std::vec;
  use std::vec::Vec;

  const NUM_SAMPLES: usize = 0x0eff;

  #[test]
  fn test_encode_frame() {
    let wav: &[i16] = &[
      -3461, -3452, -3441, -3456, -3462, -3453, -3461, -3461, -3449, -3457, -3463, -3460, -3454, -3450, -3449, -3452,
      -3450, -3449, -3463, -3462, -3453, -3458, -3448, -3447, -3461, -3462, -3452, -3452, -3456, -3459, -3456, -3449,
      -3444, -3451, -3455, -3449, -3453, -3456, -3455, -3458, -3457, -3450, -3453, -3458, -3456, -3458, -3461, -3451,
      -3447, -3450, -3461, -3459, -3450, -3450, -3453, -3464, -3463, -3455, -3452, -3457, -3453, -3453, -3453, -3445,
      -3449, -3450, -3450, -3451, -3452, -3453, -3455, -3455, -3457, -3455, -3452, -3460, -3458, -3463, -3454, -3453,
      -3458, -3461, -3470, -3464, -3447, -3449, -3456, -3462, -3457, -3449, -3455, -3456, -3448, -3444, -3449, -3442,
      -3442, -3459, -3462, -3456, -3459, -3454, -3454, -3456, -3454, -3452, -3443, -3456, -3454, -3447, -3453, -3457,
      -3460, -3456, -3447, -3451, -3459, -3460, -3462, -3459, -3444, -3443, -3445, -3450, -3459, -3451, -3443, -3450,
      -3452, -3459, -3457, -3456, -3452, -3461, -3467, -3457, -3452, -3449, -3456, -3460, -3454, -3455, -3448, -3439,
      -3445, -3454, -3450, -3440, -3457, -3472, -3458, -3444, -3451, -3458, -3463, -3460, -3456, -3467, -3463, -3450,
      -3451, -3449, -3445, -3442, -3445, -3453, -3453, -3442, -3454, -3455, -3460, -3458, -3462, -3479, -3453, -3445,
      -3456, -3458, -3442, -3442, -3445, -3441, -3448, -3449, -3443, -3453, -3452, -3450, -3468, -3467, -3451, -3458,
      -3462, -3453, -3459, -3468, -3453, -3452, -3464, -3453, -3454, -3462, -3457, -3452, -3452, -3455, -3463, -3478,
      -3469, -3464, -3472, -3461, -3465, -3461, -3460, -3455, -3456, -3454, -3449, -3455, -3460, -3464, -3456, -3462,
      -3465, -3458, -3461, -3475, -3477, -3479, -3471, -3470, -3466, -3455, -3457, -3463, -3454, -3442, -3449, -3455,
      -3453, -3454, -3456, -3465, -3466, -3460, -3454, -3459, -3460, -3460, -3464, -3457, -3458, -3459, -3459, -3467,
      -3468, -3460, -3455, -3466, -3467, -3471, -3466, -3463, -3463, -3465, -3464, -3456, -3450, -3448, -3449, -3456,
      -3462, -3456, -3462, -3461, -3463, -3468, -3462, -3464, -3462, -3460, -3470, -3467, -3458, -3457, -3465, -3469,
      -3465, -3461, -3456, -3460, -3459, -3458, -3461, -3463, -3461, -3462, -3465, -3465, -3461, -3462, -3459, -3464,
      -3458, -3461, -3464, -3460, -3474, -3470, -3473, -3473, -3470, -3465, -3459, -3460, -3464, -3469, -3457, -3462,
      -3468, -3458, -3473, -3474, -3463, -3466, -3464, -3472, -3465, -3453, -3471, -3460, -3470, -3477, -3471, -3466,
      -3462, -3462, -3468, -3465, -3459, -3469, -3466, -3465, -3466, -3473, -3468, -3465, -3469, -3465, -3460, -3455,
      -3466, -3468, -3460, -3461, -3453, -3460, -3470, -3475, -3472, -3474, -3467, -3469, -3478, -3469, -3469, -3474,
      -3460, -3454, -3462, -3458, -3461, -3467, -3467, -3460, -3470, -3471, -3477, -3472, -3468, -3470, -3466, -3468,
      -3461, -3461, -3467, -3464, -3470, -3479, -3471, -3477, -3474, -3472, -3476, -3478, -3470, -3473, -3475, -3466,
      -3461, -3463, -3468, -3466, -3463, -3465, -3470, -3468, -3466, -3465, -3472, -3479, -3476, -3474, -3475, -3466,
      -3458, -3463, -3464, -3465, -3466, -3468, -3468, -3468, -3465, -3461, -3470, -3466, -3460, -3470, -3468, -3468,
      -3463, -3465, -3472, -3470, -3476, -3479, -3468, -3473, -3484, -3455, -3468, -3475, -3444, -3459, -3470, -3471,
      -3469, -3469, -3464, -3469, -3472, -3464, -3472, -3465, -3459, -3477, -3470, -3468, -3475, -3468, -3468, -3474,
      -3470, -3466, -3470, -3480, -3469, -3457, -3472, -3476, -3466, -3456, -3471, -3475, -3468, -3472, -3471, -3470,
      -3468, -3458, -3468, -3481, -3480, -3470, -3456, -3452, -3465, -3475, -3480, -3472, -3474, -3474, -3484, -3480,
      -3471, -3480, -3480, -3473, -3467, -3465, -3464, -3458, -3461, -3465, -3465, -3475, -3483, -3476, -3479, -3479,
      -3472, -3473, -3477, -3473, -3472, -3481, -3473, -3465, -3464, -3463, -3465, -3470, -3475, -3473, -3474, -3467,
      -3468, -3485, -3483, -3473, -3475, -3473, -3465, -3467, -3475, -3482, -3476, -3470, -3473, -3474, -3477, -3474,
      -3465, -3465, -3467, -3471, -3471, -3481, -3489, -3479, -3472, -3476, -3478, -3477, -3467, -3469, -3485, -3477,
      -3471, -3480, -3471, -3482, -3475, -3465, -3462, -3470, -3473, -3469, -3459, -3476, -3484, -3475, -3470, -3470,
      -3476, -3481, -3488, -3487, -3478, -3478, -3484, -3479, -3482, -3482, -3480, -3488, -3479, -3459, -3469, -3480,
      -3486, -3472, -3457, -3455, -3476, -3492, -3474, -3465, -3475, -3479, -3474, -3478, -3490, -3488, -3482, -3482,
      -3478, -3478, -3482, -3471, -3473, -3470, -3463, -3474, -3473, -3454, -3463, -3484, -3479, -3481, -3475, -3484,
      -3475, -3476, -3476, -3460, -3461, -3466, -3465, -3465, -3469, -3481, -3481, -3475, -3476, -3474, -3484, -3482,
      -3480, -3484, -3482, -3478, -3478, -3487, -3463, -3458, -3479, -3472, -3468, -3457, -3463, -3481, -3478, -3478,
      -3467, -3478, -3490, -3478, -3476, -3475, -3469, -3480, -3485, -3470, -3469, -3476, -3466, -3464, -3466, -3476,
      -3478, -3487, -3477, -3488, -3490, -3486, -3468, -3462, -3479, -3475, -3459, -3457, -3469, -3466, -3468, -3473,
      -3476, -3473, -3478, -3489, -3490, -3484, -3474, -3482, -3492, -3470, -3470, -3472, -3473, -3477, -3474, -3478,
      -3480, -3470, -3472, -3479, -3474, -3486, -3490, -3488, -3489, -3480, -3468, -3476, -3481, -3478, -3475, -3477,
      -3472, -3472, -3474, -3478, -3473, -3484, -3495, -3486, -3477, -3488, -3494, -3493, -3488, -3484, -3486, -3484,
      -3480, -3479, -3477, -3476, -3487, -3481, -3473, -3479, -3473, -3470, -3479, -3474, -3476, -3482, -3480, -3482,
      -3485, -3488, -3490, -3487, -3477, -3483, -3494, -3490, -3479, -3488, -3499, -3490, -3484, -3482, -3483, -3480,
      -3475, -3472, -3476, -3475, -3477, -3479, -3475, -3473, -3476, -3483, -3491, -3489, -3479, -3480, -3488, -3489,
      -3491, -3486, -3488, -3484, -3475, -3487, -3502, -3498, -3481, -3476, -3490, -3499, -3487, -3476, -3484, -3480,
      -3475, -3480, -3496, -3497, -3480, -3476, -3478, -3483, -3488, -3484, -3492, -3496, -3490, -3497, -3494, -3487,
      -3497, -3498, -3493, -3491, -3497, -3500, -3493, -3494, -3496, -3490, -3488, -3485, -3490, -3491, -3492, -3489,
      -3486, -3487, -3485, -3488, -3493, -3495, -3494, -3486, -3486, -3488, -3494, -3499, -3503, -3499, -3495, -3485,
      -3492, -3509, -3503, -3494, -3504, -3496, -3491, -3496, -3490, -3498, -3493, -3482, -3481, -3498, -3500, -3496,
      -3494, -3502, -3497, -3499, -3495, -3485, -3480, -3490, -3494, -3496, -3488, -3494, -3501, -3512, -3501, -3496,
      -3510, -3511, -3493, -3494, -3487, -3501, -3502, -3467, -3483, -3506, -3500, -3491, -3501, -3483, -3490, -3495,
      -3500, -3495, -3492, -3493, -3490, -3496, -3499, -3481, -3482, -3492, -3488, -3498, -3499, -3496, -3492, -3488,
      -3495, -3494, -3483, -3491, -3492, -3495, -3491, -3489, -3499, -3492, -3479, -3482, -3486, -3479, -3489, -3501,
      -3493, -3489, -3489, -3488, -3485, -3481, -3495, -3495, -3486, -3489, -3496, -3494, -3496, -3493, -3491, -3488,
      -3499, -3500, -3492, -3491, -3490, -3498, -3495, -3495, -3497, -3487, -3485, -3488, -3482, -3485, -3487, -3487,
      -3483, -3496, -3503, -3495, -3482, -3489, -3492, -3501, -3511, -3491, -3495, -3500, -3490, -3492, -3497, -3504,
      -3503, -3497, -3493, -3494, -3489, -3495, -3492, -3483, -3493, -3493, -3490, -3490, -3504, -3499, -3501, -3499,
      -3487, -3496, -3501, -3497, -3493, -3492, -3491, -3492,
    ];
    let wl = wav.len();
    let wlh = (wl >> 8) as u8;
    let wll = (wl & 0xff) as u8;

    let expected_x3_output: &[u8] = &[
      // Frame header
      'x' as u8, '3' as u8, // "x3"
      1, 1, // Source Id, Num Channels
      wlh, wll, // Num samples
      2, 144, // Num encoded bytes
      0, 0, 0, 0, 0, 0, 0, 0, // Time
      110, 210, // Header CRC
      61, 223, // Payload CRC
      // Frame payload
      242, 123, 202, 56, 106, 202, 124, 8, 122, 249, 136, 173, 202, 23, 69, 105, 50, 133, 201, 145, 251, 206, 83, 125,
      159, 181, 181, 187, 83, 151, 166, 35, 77, 194, 163, 77, 162, 57, 40, 226, 8, 249, 137, 153, 184, 188, 231, 226,
      74, 164, 250, 75, 2, 173, 171, 81, 11, 146, 16, 151, 64, 78, 179, 117, 69, 230, 20, 39, 28, 191, 212, 21, 239,
      155, 225, 221, 90, 73, 32, 134, 219, 114, 144, 74, 204, 87, 55, 217, 203, 138, 89, 40, 48, 36, 104, 193, 141,
      106, 121, 6, 160, 84, 138, 16, 129, 251, 192, 5, 233, 255, 97, 121, 123, 68, 107, 242, 0, 123, 161, 178, 194,
      235, 2, 232, 33, 190, 82, 201, 29, 71, 157, 59, 212, 168, 215, 141, 41, 79, 59, 209, 74, 156, 82, 211, 249, 11,
      215, 104, 187, 178, 41, 7, 90, 202, 16, 213, 231, 54, 76, 177, 137, 152, 247, 76, 195, 228, 133, 27, 48, 193, 10,
      104, 2, 2, 44, 12, 24, 16, 52, 195, 2, 101, 225, 124, 85, 63, 208, 133, 125, 86, 222, 78, 194, 57, 228, 179, 117,
      2, 209, 177, 202, 100, 250, 68, 37, 121, 131, 100, 237, 199, 184, 64, 89, 197, 181, 138, 68, 47, 153, 151, 213,
      45, 87, 122, 20, 163, 108, 137, 33, 165, 167, 187, 117, 146, 84, 36, 99, 30, 141, 92, 56, 185, 90, 145, 218, 45,
      208, 190, 89, 36, 47, 179, 236, 141, 178, 149, 218, 115, 173, 57, 148, 214, 246, 73, 68, 38, 102, 123, 136, 228,
      18, 134, 23, 144, 171, 110, 47, 78, 73, 162, 187, 159, 47, 227, 175, 225, 0, 47, 126, 145, 192, 227, 92, 56, 92,
      238, 133, 161, 30, 89, 194, 6, 249, 134, 13, 247, 125, 86, 102, 22, 38, 140, 24, 129, 37, 164, 139, 130, 208, 81,
      48, 230, 121, 76, 223, 130, 222, 237, 135, 79, 162, 137, 32, 138, 234, 210, 156, 151, 72, 78, 25, 120, 69, 189,
      172, 102, 243, 188, 84, 95, 193, 111, 49, 223, 116, 101, 143, 33, 153, 37, 26, 227, 57, 250, 131, 2, 94, 82, 161,
      105, 109, 20, 40, 29, 11, 232, 1, 112, 74, 155, 107, 209, 199, 133, 94, 9, 19, 176, 190, 11, 230, 129, 12, 0,
      128, 120, 95, 193, 143, 168, 41, 239, 88, 189, 176, 146, 166, 2, 37, 53, 30, 124, 50, 112, 188, 193, 124, 8, 64,
      55, 96, 90, 199, 16, 190, 174, 12, 2, 245, 208, 192, 186, 97, 169, 29, 77, 51, 44, 187, 36, 195, 109, 1, 140, 9,
      20, 68, 2, 208, 63, 187, 244, 62, 245, 252, 98, 184, 217, 96, 62, 255, 254, 251, 50, 218, 163, 255, 36, 161, 15,
      79, 117, 171, 139, 245, 26, 105, 69, 26, 186, 82, 47, 17, 89, 70, 236, 33, 108, 226, 85, 107, 203, 221, 190, 49,
      102, 160, 236, 147, 74, 102, 79, 43, 190, 174, 209, 154, 215, 225, 156, 190, 109, 86, 130, 143, 26, 128, 165, 11,
      36, 65, 207, 66, 180, 143, 144, 164, 90, 82, 135, 251, 27, 206, 46, 85, 139, 140, 5, 176, 34, 16, 126, 33, 100,
      97, 180, 1, 93, 173, 62, 132, 24, 208, 150, 20, 89, 10, 75, 30, 163, 171, 9, 180, 99, 213, 104, 49, 69, 253, 144,
      182, 154, 114, 133, 141, 252, 151, 240, 252, 191, 163, 225, 164, 48, 158, 196, 188, 251, 246, 20, 31, 240, 122,
      244, 50, 75, 65, 115, 200, 67, 104, 231, 206, 163, 11, 220, 43, 125, 197, 158, 66, 34, 185, 2, 224, 173, 110, 95,
      217, 198, 201, 21, 79, 232, 179, 51, 89, 183, 8, 196, 180, 129, 77, 210, 75, 4, 122, 76, 180, 182, 152, 137, 86,
      190, 40, 184, 232, 22, 171, 193, 4, 165, 8, 170, 144, 0,
    ];
    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();
    let stats: &mut [usize; 6] = &mut [0; 6];

    encode_frame(wav, bp, params, stats).unwrap();

    assert_eq!(expected_x3_output, bp.as_bytes());
  }

  #[test]
  fn test_encode_frame_zeros() {
    let wav: &[i16] = &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let wl = wav.len();
    let wlh = (wl >> 8) as u8;
    let wll = (wl & 0xff) as u8;
    let expected_x3_output: &[u8] = &[
      // Frame header
      b'x', b'3', // "x3"
      1, 1, // Source Id, Num Channels
      wlh, wll, // Num samples
      0, 6, // Num encoded bytes
      0, 0, 0, 0, 0, 0, 0, 0, // Time
      194, 242, // Header CRC
      205, 128, // Payload CRC
      // Frame payload
      0, 0, 127, 255, 248, 0,
    ];
    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();
    let stats: &mut [usize; 6] = &mut [0; 6];

    encode_frame(wav, bp, params, stats).unwrap();

    assert_eq!(expected_x3_output, bp.as_bytes());
  }

  #[test]
  fn test_x3_encode_block() {
    let wav: &[i16] = &[
      -3461, -3452, -3441, -3456, -3462, -3453, -3461, -3461, -3449, -3457, -3463, -3460, -3454, -3450, -3449, -3452,
      -3450, -3449, -3463, -3462, -3462,
    ];
    let mut wav_diff = diff(wav);

    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2 + 1];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();

    // Run the code
    x3_encode_block(&wav[1..], &mut wav_diff, bp, params).unwrap();
    bp.word_align();

    // Check output is okay
    let expected_x3_output: &[u8] = &[202, 56, 106, 202, 124, 8, 122, 249, 136, 173, 202, 23, 80, 0];

    assert_eq!(expected_x3_output, bp.as_bytes());
  }

  #[test]
  fn test_x3_encode_block_ftype3() {
    let wav: &[i16] = &[
      -3554,
      -3559,
      -3566,
      -3563,
      -3553,
      -3547,
      -3543,
      -3552,
      -3564,
      -3563,
      -3558,
      -3558,
      -3557,
      -3547,
      -3549,
      -3552,
      -3554,
      -3556,
      -3566,
      -3584,
      -3584 + 11,
    ];
    let mut wav_diff = diff(wav);

    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2 + 1];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();

    // Run the code
    bp.write_packed_zeros(1);
    x3_encode_block(&wav[1..], &mut wav_diff, bp, params).unwrap();
    bp.word_align();

    // Check output is okay
    let expected_x3_output: &[u8] = &[105, 111, 24, 196, 18, 125, 42, 40, 203, 219, 178, 194, 206, 0];

    assert_eq!(expected_x3_output, bp.as_bytes());
  }

  #[test]
  fn test_x3_encode_block_bpf_eq16() {
    let wav: &[i16] = &[
      -32341, -16767, 4562, -1601, 9638, 22598, 14100, -12957, -10471, 29926, -14190, 31863, 29234, -16603, 31762,
      1319, 11044, -28931, 17888, -14247, -14247,
    ];
    let mut wav_diff = diff(wav);

    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2 + 1];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();

    // Run the code
    x3_encode_block(&wav[1..], &mut wav_diff, bp, params).unwrap();
    bp.word_align();

    // Check output is okay
    let expected_x3_output: &[u8] = &[
      62, 250, 4, 71, 75, 230, 252, 150, 153, 97, 24, 220, 83, 53, 143, 92, 101, 211, 155, 34, 73, 241, 221, 200, 202,
      252, 149, 240, 72, 20, 156, 172, 146, 59, 245, 23, 131, 33, 103, 33, 100, 0,
    ];

    assert_eq!(expected_x3_output, bp.as_bytes());
  }

  #[test]
  fn test_x3_encode_block_bpf_lt16() {
    let wav: &[i16] = &[
      -3511, -3493, -3494, -3487, -3501, -3502, -3467, -3483, -3506, -3500, -3491, -3501, -3483, -3490, -3495, -3500,
      -3495, -3492, -3493, -3490, -3490,
    ];
    let mut wav_diff = diff(wav);

    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2 + 1];
    let bp = &mut BitPacker::new(x3_output);
    let params = &Parameters::default();

    // Run the code
    x3_encode_block(&wav[1..], &mut wav_diff, bp, params).unwrap();
    bp.word_align();

    // Check output is okay
    let expected_x3_output: &[u8] = &[
      24, 151, 240, 252, 191, 163, 225, 164, 48, 158, 196, 188, 251, 246, 20, 31, 240, 96,
    ];

    assert_eq!(expected_x3_output, &x3_output[0..expected_x3_output.len()],);
  }

  #[test]
  fn test_x3_encode_samples() {
    let wav: Vec<i16> = vec![0; 1000];

    // Can only handle signed 16 bit data with one channel.
    let params = x3::Parameters::default();
    let sample_rate = 44100;
    let num_samples = wav.len();

    // Create the channel data
    let mut first_channel = x3::IterChannel::new(0, wav.into_iter(), sample_rate, params);

    // Create the output data
    let x3_len = num_samples * 2;
    let mut x3_out = vec![0u8; x3_len];
    let bp = &mut BitPacker::new(&mut x3_out); // Packer where x3 compressed data is stored.

    encoder::encode(&mut [&mut first_channel], bp).unwrap();

    // Get the bytes
    let _x3_bytes = bp.as_bytes();
  }
}
