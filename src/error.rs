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

// We derive `Debug` because all types should probably derive `Debug`.
// This gives us a reasonable human readable description of `CliError` values.
#[derive(Debug)]
pub enum X3Error {
  Io(std::io::Error),
  Hound(hound::Error),
  BitPack(crate::bitpack::BitPackError),

  // Custom X3 Errors
  InvalidEncodingThresh, // Threshold must be less than or equal to code.offset
  InvalidArrayLen,       // Array lengths are not compatible.
  OutOfBoundsInverse,    // The value is out-of-bounds for the .inv array.
  MoreThanOneChannel,    // FIXME: We need to support more than one channel

  // X3 Archive Header errors
  ArchiveHeaderXMLInvalid,    // XML is poorly structured
  ArchiveHeaderXMLRiceCode,   // XML has invalid rice code
  ArchiveHeaderXMLInvalidKey, // Invalid archive key 'X3ARHIV'

  // Frame issues
  FrameLength, // The frame is too long

  // Frame header issues
  FrameHeaderInvalidKey,        // The frame header is missing 'x3'
  FrameHeaderInvalidPayloadLen, // The payload length reaches beyond the end of the available data
  FrameHeaderInvalidHeaderCRC,
  FrameHeaderInvalidPayloadCRC,

  // Decoding issues
  FrameDecodeInvalidBlockLength, // The block length is bad
  FrameDecodeInvalidNTOGO,       // Invalid ntogo
  FrameDecodeInvalidRiceCode,    // The Rice codes are invalid
  FrameDecodeInvalidBPF,         // The BPF decoder blew up, an invalid value was reached.
  FrameDecodeUnexpectedEnd,      // The BitReader has less bytes than the size of the header, but still expects a frame.
}

impl From<std::io::Error> for X3Error {
  fn from(err: std::io::Error) -> X3Error {
    X3Error::Io(err)
  }
}

impl From<hound::Error> for X3Error {
  fn from(err: hound::Error) -> X3Error {
    X3Error::Hound(err)
  }
}

impl From<crate::bitpack::BitPackError> for X3Error {
  fn from(err: crate::bitpack::BitPackError) -> X3Error {
    X3Error::BitPack(err)
  }
}
