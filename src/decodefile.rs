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
#[cfg(feature = "oceaninstruments")]
use crate::byteorder::{BigEndian, ByteOrder};
use crate::hound;

// this crate
use crate::bitpack::ByteReader;
use crate::decoder;
use crate::error;
use crate::x3;

use error::X3Error;
use quick_xml::events::Event;
use quick_xml::Reader;

///
/// Convert an .x3a (X3 Archive) file to a .wav file.  
///
/// Note: the x3a can contain some meta data of the recording that may be lost, such as the time
///       of the recording and surplus XML payload data that has been embedded into the X3A header.
///
/// ### Arguments
///
/// * `x3a_filename` - the input X3A file to decode.
/// * `wav_filename` - the output wav file to write to.  It will be overwritten.
///
pub fn x3a_to_wav<P: AsRef<path::Path>>(x3a_filename: P, wav_filename: P) -> Result<(), X3Error> {
  let mut file = File::open(x3a_filename).unwrap();

  let mut buf = Vec::new();
  file.read_to_end(&mut buf).unwrap();
  let bytes = &mut ByteReader::new(&buf);
  let (sample_rate, params) = read_archive_header(bytes).expect("Invalid X3 Archive header");
  let wav = decoder::decode_frames(bytes, &params)?;

  let spec = hound::WavSpec {
    channels: 1,
    sample_rate: sample_rate as u32,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };

  let mut writer = hound::WavWriter::create(wav_filename, spec)?;
  for w in wav {
    writer.write_sample(w)?;
  }

  Ok(())
}

///
/// Convert an .bin (x3 binary without archive details) file to a .wav file.  
///
/// ### Arguments
///
/// * `x3bin_filename` - the input x3 bin (.bin) file to decode.
/// * `wav_filename` - the output wav file to write to.  It will be overwritten.
///
pub fn x3bin_to_wav<P: AsRef<path::Path>>(x3bin_filename: P, wav_filename: P) -> Result<(), X3Error> {
  let mut file = File::open(x3bin_filename).unwrap();

  let mut buf = Vec::new();
  file.read_to_end(&mut buf).unwrap();
  let bytes = &mut ByteReader::new(&mut buf);

  let (sample_rate, params) = read_header(bytes)?;

  // For OceanInstruments, the file header is the first frame.
  #[cfg(feature = "oceaninstruments")]
  {
    bytes.reset();
  }

  println!("sample_rate: {}\nblock_len: {}", sample_rate, params.block_len);
  let wav = decoder::decode_frames(bytes, &params)?;

  let spec = hound::WavSpec {
    channels: 1,
    sample_rate: sample_rate as u32,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };

  let mut writer = hound::WavWriter::create(wav_filename, spec)?;
  for w in wav {
    writer.write_sample(w)?;
  }

  Ok(())
}

///
/// Read the frame header to the ByteReader output.
///
fn read_header(bytes: &mut ByteReader) -> Result<(u32, x3::Parameters), X3Error> {
  let buf = &mut [0u8; x3::FrameHeader::HEADER_CRC_BYTE];
  bytes.read(buf)?;

  #[cfg(not(feature = "oceaninstruments"))]
  let sample_rate = 48000; // FIXME: Need to set this somehow else

  #[cfg(feature = "oceaninstruments")]
  let sample_rate = BigEndian::read_u16(&buf[x3::FrameHeader::SAMPLE_RATE_BYTE..]);

  let params = x3::Parameters::default();

  Ok((sample_rate as u32, params))
}

///
/// Read <Archive Header> to the BitReader output.
///
fn read_archive_header(bytes: &mut ByteReader) -> Result<(u32, x3::Parameters), X3Error> {
  // <Archive Id>
  if !bytes.eq(x3::Archive::ID)? {
    return Err(X3Error::ArchiveHeaderXMLInvalidKey);
  }
  bytes.inc_counter(x3::Archive::ID.len())?;

  // <XML MetaData>
  let (_, payload_size) = decoder::read_frame_header(bytes)?;

  // Get the payload
  let mut payload: Vec<u8> = vec![0; payload_size];
  bytes.read(&mut payload)?;

  let xml = String::from_utf8_lossy(&payload);

  let (sample_rate, params) = parse_xml(&xml)?;

  Ok((sample_rate, params))
}

///
/// Parse the XML header that contains the parameters for the wav output.
///
fn parse_xml(xml: &str) -> Result<(u32, x3::Parameters), X3Error> {
  let mut reader = Reader::from_str(xml);
  reader.trim_text(true);

  let mut buf = Vec::new();
  let mut fs = Vec::with_capacity(3);
  let mut bl = Vec::with_capacity(3);
  let mut codes = Vec::with_capacity(3);
  let mut th = Vec::with_capacity(3);

  // The `Reader` does not implement `Iterator` because it outputs borrowed data (`Cow`s)
  loop {
    match reader.read_event(&mut buf) {
      Ok(Event::Start(ref e)) => match e.name() {
        b"FS" => fs.push(reader.read_text(e.name(), &mut Vec::new()).unwrap()),
        b"BLKLEN" => bl.push(reader.read_text(e.name(), &mut Vec::new()).unwrap()),
        b"CODES" => codes.push(reader.read_text(e.name(), &mut Vec::new()).unwrap()),
        b"T" => th.push(reader.read_text(e.name(), &mut Vec::new()).unwrap()),
        _ => (),
      },
      Ok(Event::Eof) => break, // exits the loop when reaching end of file
      Err(e) => {
        println!(
          "Error reading X3 Archive header (XML) at position {}: {:?}",
          reader.buffer_position(),
          e
        );
        return Err(X3Error::ArchiveHeaderXMLInvalid);
      }
      _ => (), // There are several other `Event`s we do not consider here
    }

    // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
    buf.clear();
  }
  println!("sample rate: {}", fs[0]);
  println!("block length: {}", bl[0]);
  println!("Rice codes: {}", codes[0]);
  println!("thresholds: {}", th[0]);

  let sample_rate = fs[0].parse::<u32>().unwrap();
  let block_len = bl[0].parse::<u32>().unwrap();
  let mut rice_code_ids = Vec::new();
  for word in codes[0].split(',') {
    match word {
      "RICE0" => rice_code_ids.push(0),
      "RICE1" => rice_code_ids.push(1),
      "RICE2" => rice_code_ids.push(2),
      "RICE3" => rice_code_ids.push(3),
      "BFP" => (),
      _ => return Err(X3Error::ArchiveHeaderXMLRiceCode),
    };
  }
  let thresholds: Vec<usize> = th[0].split(',').map(|s| s.parse::<usize>().unwrap()).collect();

  let mut rc_array: [usize; 3] = [0; 3];
  let mut th_array: [usize; 3] = [0; 3];

  #[allow(clippy::manual_memcpy)]
  for i in 0..3 {
    rc_array[i] = rice_code_ids[i];
    th_array[i] = thresholds[i];
  }
  let params = x3::Parameters::new(
    block_len as usize,
    x3::Parameters::DEFAULT_BLOCKS_PER_FRAME,
    rc_array,
    th_array,
  )?;

  Ok((sample_rate, params))
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

// #[cfg(test)]
// mod tests {
//   use crate::decodefile::x3a_to_wav;

//   #[test]
//   fn test_decodefile() {
//     x3a_to_wav("~/x3/rust/test.x3a", "~/x3/rust/test-test.wav").unwrap();
//   }
// }
