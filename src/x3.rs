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

use crate::error::X3Error;

pub struct Decoder<'a> {
  pub channels: &'a [Channel<'a>],
  pub x3_inp: &'a mut [u8],
}

pub struct Channel<'a> {
  pub id: u16,            // The channel number
  pub wav: &'a [i16],     // The raw wave data
  pub sample_rate: u32,   // The sample rate in Hz
  pub params: Parameters, // X3 encoding parameters
}

impl<'a> Channel<'a> {
  pub fn new(id: u16, wav: &'a [i16], sample_rate: u32, params: Parameters) -> Self {
    Channel {
      id,
      wav,
      sample_rate,
      params,
    }
  }
}

pub struct IterChannel<I>
where
  I: Iterator<Item = i16>,
{
  pub id: u16,            // The channel number
  pub wav: I,             // Raw sample iterator
  pub sample_rate: u32,   // The sample rate in Hz
  pub params: Parameters, // X3 encoding parameters
}

impl<I> IterChannel<I>
where
  I: Iterator<Item = i16>,
{
  pub fn new(id: u16, wav: impl IntoIterator<IntoIter = I>, sample_rate: u32, params: Parameters) -> Self {
    IterChannel {
      id,
      wav: wav.into_iter(),
      sample_rate,
      params,
    }
  }
}
pub struct X3aSpec {
  /// The number of samples per second.
  pub sample_rate: u32,

  /// The parameters for the stream
  pub params: Parameters,

  /// The number of channels in use
  pub channels: u8,
}

pub struct Parameters {
  pub channel_count: usize,
  pub block_len: usize,
  pub blocks_per_frame: usize,
  pub codes: [usize; 3],
  pub thresholds: [usize; 3],
  pub rice_codes: [&'static RiceCode; 3],
}

impl Parameters {
  pub const MAX_CHANNEL_COUNT: usize = 8;
  pub const MAX_BLOCK_LENGTH: usize = 60;
  pub const WAV_BIT_SIZE: usize = 16;

  pub const DEFAULT_CHANNEL_COUNT: usize = 1;
  pub const DEFAULT_BLOCK_LENGTH: usize = 20;
  pub const DEFAULT_RICE_CODES: [usize; 3] = [0, 1, 3];
  pub const DEFAULT_THRESHOLDS: [usize; 3] = [3, 8, 20];
  pub const DEFAULT_BLOCKS_PER_FRAME: usize = 500;

  pub fn new(
    block_len: usize,
    blocks_per_frame: usize,
    codes: [usize; 3],
    thresholds: [usize; 3],
  ) -> Result<Self, X3Error> {
    let rice_codes = RiceCodes::get(codes);

    // setup the codes
    for k in 0..2 {
      let rc = rice_codes[k];
      if thresholds[k] > rc.offset {
        return Err(X3Error::InvalidEncodingThresh);
      }
    }

    Ok(Parameters {
      block_len,
      blocks_per_frame,
      codes,
      thresholds,
      rice_codes,
      ..Default::default()
    })
  }
}

impl Default for Parameters {
  fn default() -> Self {
    Parameters {
      channel_count: 1,
      block_len: Self::DEFAULT_BLOCK_LENGTH,
      blocks_per_frame: Self::DEFAULT_BLOCKS_PER_FRAME,
      codes: Self::DEFAULT_RICE_CODES,
      thresholds: Self::DEFAULT_THRESHOLDS,
      rice_codes: RiceCodes::get(Self::DEFAULT_RICE_CODES),
    }
  }
}

pub struct Archive {}
impl Archive {
  /// <Archive Id>
  pub const ID: &'static [u8] = &[0x58, 0x33, 0x41, 0x52, 0x43, 0x48, 0x49, 0x56]; // 'X3ARCHIV'
  pub const ID_LEN: usize = 8;
}

pub struct Frame {}
impl Frame {
  pub const MAX_LENGTH: usize = 0x7fe0;
}

pub struct FrameHeader {
  /// Source Id of the stream
  pub source_id: u8,

  /// The number of samples in the frame
  pub samples: u16,

  /// The number of channels
  pub channels: u8,

  /// The length of the frame (bytes)
  pub payload_len: usize,

  /// The CRC16 value for the payload
  pub payload_crc: u16,
}

impl FrameHeader {
  /// The length of the header
  pub const LENGTH: usize = 20;

  /// Fixed key marks the boundary of the frame 'x3'
  pub const KEY: u16 = 30771; // "x3"
  pub const KEY_BUF: &'static [u8] = &[0x78, 0x33]; // "x3"

  /// The location of various bytes in the header
  pub const P_KEY: usize = 0;
  pub const P_SOURCE_ID: usize = 2;
  pub const P_CHANNELS: usize = 3;
  pub const P_SAMPLES: usize = 4;
  pub const P_PAYLOAD_SIZE: usize = 6;
  pub const P_TIME: usize = 8;

  /// CRC of the encoded payload, all the frames
  pub const P_HEADER_CRC: usize = 16;
  pub const P_PAYLOAD_CRC: usize = 18;
}

#[allow(dead_code)]
pub struct RiceCode {
  pub nsubs: usize,  // number of subcode (suffix) bits
  pub offset: usize, // table offset
  pub code: &'static [usize],
  pub num_bits: &'static [usize],
  pub inv: &'static [i16],
  pub inv_len: usize, // The length of inv that is used for this.
}

pub struct RiceCodes {}

// The inverse rice code lookup table is the same for all rice codes, although the lower valued
// rice codes need less than what is provided here
const INV_RICE_CODE: &[i16] = &[
  0, -1, 1, -2, 2, -3, 3, -4, 4, -5, 5, -6, 6, -7, 7, -8, 8, -9, 9, -10, 10, -11, 11, -12, 12, -13, 13, -14, 14, -15,
  15, -16, 16, -17, 17, -18, 18, -19, 19, -20, 20, -21, 21, -22, 22, -23, 23, -24, 24, -25, 25, -26, 26, -27, 27, -28,
  28, -29, 29, -30,
];

impl RiceCodes {
  const CODE: [RiceCode; 4] = [
    RiceCode {
      nsubs: 0,
      offset: 6,
      code: &[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
      num_bits: &[12, 10, 8, 6, 4, 2, 1, 3, 5, 7, 9, 11, 13, 15],
      inv: INV_RICE_CODE,
      inv_len: 16,
    },
    RiceCode {
      nsubs: 1,
      offset: 11,
      code: &[3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
      num_bits: &[12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
      inv: INV_RICE_CODE,
      inv_len: 26,
    },
    RiceCode {
      nsubs: 2,
      offset: 20,
      code: &[
        7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 7, 5, 4, 6, 4, 6, 4, 6, 4, 6, 4, 6, 4, 6, 4, 6, 4, 6, 4,
        6, 4, 6,
      ],
      num_bits: &[
        12, 12, 11, 11, 10, 10, 9, 9, 8, 8, 7, 7, 6, 6, 5, 5, 4, 4, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10,
        10, 11, 11, 12, 12,
      ],
      inv: INV_RICE_CODE,
      inv_len: 44,
    },
    RiceCode {
      nsubs: 3,
      offset: 28,
      code: &[
        15, 13, 11, 9, 15, 13, 11, 9, 15, 13, 11, 9, 15, 13, 11, 9, 15, 13, 11, 9, 15, 13, 11, 9, 15, 13, 11, 9, 8, 10,
        12, 14, 8, 10, 12, 14, 8, 10, 12, 14, 8, 10, 12, 14, 8, 10, 12, 14, 8, 10, 12, 14, 8, 10, 12, 14,
      ],
      num_bits: &[
        10, 10, 10, 10, 9, 9, 9, 9, 8, 8, 8, 8, 7, 7, 7, 7, 6, 6, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5,
        6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8, 9, 9, 9, 9, 10, 10, 10, 10,
      ],
      inv: INV_RICE_CODE,
      inv_len: 60,
    },
  ];

  pub fn get(code_list: [usize; 3]) -> [&'static RiceCode; 3] {
    [
      &RiceCodes::CODE[code_list[0]],
      &RiceCodes::CODE[code_list[1]],
      &RiceCodes::CODE[code_list[2]],
    ]
  }
}
