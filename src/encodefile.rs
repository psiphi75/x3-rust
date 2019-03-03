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

// std
use std::fs::File;
use std::io::prelude::*;
use std::path;

// externs
use crate::hound;

// this crate
use crate::bitpack::BitPacker;
use crate::encoder;
use crate::error;
use crate::x3;

use error::X3Error;

///
/// Convert a .wav file to an .x3a (X3 Archive) file.  
///
/// ### Arguments
///
/// * `wav_filename` - the input wav file to read.
/// * `x3a_filename` - the output X3A file.  It will be overwritten.
///
pub fn wav_to_x3a<P: AsRef<path::Path>>(wav_filename: P, x3a_filename: P) -> Result<(), X3Error> {
  let mut reader = hound::WavReader::open(wav_filename).unwrap();

  // Can only handle 16 bit data
  assert_eq!(reader.spec().bits_per_sample, 16);

  // FIXME: We want to be able to handle multiple channels
  assert_eq!(reader.spec().channels, 1);

  let params = x3::Parameters::default();
  let sample_rate = reader.spec().sample_rate;

  // FIXME: This is pretty memory inefficient.  Should process bit by bit
  let mut samples = reader.samples::<i16>().map(|x| x.unwrap()).collect::<Vec<i16>>();
  let first_channel = x3::Channel::new(0, &mut samples[0..], sample_rate, params);

  let num_samples = first_channel.wav.len();
  let mut x3_out = vec![0u8; num_samples * 2];

  let bp = &mut BitPacker::new(&mut x3_out);

  // Output file header
  create_archive_header(&first_channel, bp)?;

  encoder::encode(&[&first_channel], bp)?;

  // Write to disk
  let mut file = File::create(x3a_filename)?;
  file.write_all(bp.as_bytes()?)?;

  Ok(())
}

//
// Write <Archive Header> to the BitPacker output.
//
fn create_archive_header(ch: &x3::Channel, bp: &mut BitPacker) -> Result<(), X3Error> {
  // <Archive Id>
  bp.write_bytes(x3::Archive::ID)?;

  // Make space for the header
  bp.bookmark();
  bp.inc_counter_n_bytes(x3::FrameHeader::LENGTH)?;

  let xml: &str = &[
    // "<X3A>",
    // "<?xml version=\"1.0\" encoding=\"US-ASCII\" ?>",
    "<X3ARCH PROG=\"x3new.m\" VERSION=\"2.0\" />",
    "<CFG ID=\"0\" FTYPE=\"XML\" />",
    "<CFG ID=\"1\" FTYPE=\"WAV\">",
    &format!("<FS UNIT=\"Hz\">{}</FS>", ch.sample_rate),
    "<SUFFIX>wav</SUFFIX>",
    "<CODEC TYPE=\"X3\" VERS=\"2\">",
    &format!("<BLKLEN>{}</BLKLEN>", ch.params.block_len),
    &format!(
      "<CODES N=\"4\">RICE{},RICE{},RICE{},BFP</CODES>",
      ch.params.codes[0], ch.params.codes[1], ch.params.codes[2]
    ),
    "<FILTER>DIFF</FILTER>",
    "<NBITS>16</NBITS>",
    &format!(
      "<T N=\"3\">{},{},{}</T>",
      ch.params.thresholds[0], ch.params.thresholds[1], ch.params.thresholds[2]
    ),
    "</CODEC>",
    "</CFG>",
    // "</X3A>",
  ]
  .concat();

  // <XML MetaData>
  bp.write_bytes(xml.as_bytes())?;
  if xml.len() % 2 == 1 {
    // Align to the nearest word
    bp.write_bits(0, 8)?;
  }

  // <Frame Header>
  encoder::write_frame_header(bp, 0, 0)?;

  Ok(())
}
