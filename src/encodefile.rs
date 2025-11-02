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
use std::format;
use std::fs::File;
use std::io::BufWriter;
use std::path;

// externs
use crate::hound;

// this crate
use crate::bytewriter::{ByteWriter, SeekFrom, StreamByteWriter};
use crate::crc::{crc16, update_crc16}; 
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

  let samples = reader.samples::<i16>().map(|x| x.unwrap());
  let mut first_channel = x3::IterChannel::new(0, samples, sample_rate, params);

  // Open output file
  // Note (MSH): BufWriter is not necessary but should improve performance as
  //       underlying BitPacker struct performs many single byte writes.
  let mut x3_output_file = File::create(x3a_filename)?;
  let mut x3_buffered_writer = BufWriter::new(x3_output_file);
  let mut x3_output_writer = StreamByteWriter::new(&mut x3_buffered_writer);
  // let mut x3_output_writer = StreamByteWriter::new(&mut x3_output_file); // if not using BufWriter
  
  // Output file header
  create_archive_header(&first_channel, &mut x3_output_writer)?;

  encoder::encode(&mut [&mut first_channel], &mut x3_output_writer)?;

  Ok(())
}

//
// Write <Archive Header> to the BitPacker output.
//
fn create_archive_header<I, W:ByteWriter>(ch: &x3::IterChannel<I>, writer: &mut W) -> Result<(), X3Error> 
  where I: Iterator<Item = i16>
{
  // <Archive Id>
  writer.write_all(x3::Archive::ID)?;

  // Make space for the header
  let frame_header_pos = writer.stream_position()?;
  writer.seek(SeekFrom::Current(x3::FrameHeader::LENGTH as i64))?;

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
  let xml_bytes = xml.as_bytes();
// <XML MetaData>
  let mut payload_len = xml_bytes.len();
  let mut payload_crc = crc16(xml_bytes);
  writer.write_all(xml_bytes)?;
  if payload_len % 2 == 1 {
    // Align to the nearest word
    writer.write_all([0u8])?;
    payload_len += 1;
    payload_crc = update_crc16(payload_crc, &0u8);
  }

  // <Frame Header>
  // Write the header details
  let return_position = writer.stream_position()?;
  writer.seek(SeekFrom::Start(frame_header_pos))?;
  let frame_header = encoder::write_frame_header(0, 0, payload_len, payload_crc);
  writer.write_all(frame_header)?;
  writer.seek(SeekFrom::Start(return_position))?;
  Ok(())
}

// #[cfg(test)]
// mod tests {
//   use crate::encodefile::wav_to_x3a;

//   #[test]
//   fn test_encodefile() {
//     wav_to_x3a("~/../../../sounds/15s/NO96_15s.wav", "~/test.wav").unwrap();
//   }
// }
