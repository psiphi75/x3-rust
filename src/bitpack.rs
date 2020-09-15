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
use crate::byteorder::{BigEndian, ByteOrder, LittleEndian};
use crate::crc::crc16;

#[derive(Debug)]
pub enum BitPackError {
  None,                // No error
  NotByteAligned,      // The bytes are not aligned.
  BoundaryReached,     // The soft boundary has been reached.
  ArrayEndReached,     // The end of the array has been reached.
  ExceededBitBoundary, // More bits were read than we expected
}

//
//      ######          ######
//      #     # # ##### #     #   ##    ####  #    # ###### #####
//      #     # #   #   #     #  #  #  #    # #   #  #      #    #
//      ######  #   #   ######  #    # #      ####   #####  #    #
//      #     # #   #   #       ###### #      #  #   #      #####
//      #     # #   #   #       #    # #    # #   #  #      #   #
//      ######  #   #   #       #    #  ####  #    # ###### #    #
//

///
/// BitPacker allows individual bits to be written to an array of bytes.
///
pub struct BitPacker<'a> {
  array: &'a mut [u8],

  // Bit pointer
  p_byte: usize,
  p_bit: usize,

  // Bookmark
  bm_p_byte: usize,
}

impl<'a> BitPacker<'a> {
  pub fn new(array: &'a mut [u8]) -> BitPacker {
    BitPacker {
      array,
      p_byte: 0,
      p_bit: 0,
      bm_p_byte: 0,
    }
  }

  ///
  /// Save the current position as a bookmark.  Later we will be
  /// able to write an array of bytes to this position.
  ///
  pub fn bookmark(&mut self) {
    self.bm_p_byte = self.p_byte;
  }

  ///
  /// Get the output array from the bookmark, to the last packed bit.
  ///
  pub fn bookmark_get_from(&self) -> Result<&[u8], BitPackError> {
    Ok(&self.array[self.bm_p_byte..self.p_byte])
  }

  ///
  /// Get number of bytes from the bookmark to the current pointer.
  ///
  pub fn bookmark_get_offset(&self) -> Result<usize, BitPackError> {
    Ok(self.p_byte - self.bm_p_byte + if self.p_bit == 0 { 0 } else { 1 })
  }

  ///
  /// Write the array from the bookmark onwards, until array is
  /// exhausted.
  ///
  pub fn bookmark_write(&mut self, array: &[u8]) -> Result<(), BitPackError> {
    for (i, value) in array.iter().enumerate() {
      self.array[self.bm_p_byte + i] = *value;
    }
    Ok(())
  }

  ///
  /// Standard write an array
  ///
  pub fn write_bytes(&mut self, array: &[u8]) -> Result<(), BitPackError> {
    for value in array {
      self.array[self.p_byte] = *value;
      self.p_byte += 1;
    }
    Ok(())
  }

  ///
  /// This operates together with `write_packed_bits`.  It only increments the
  /// `p_bit` value by 1, also incrementing `p_byte` where necessary.
  ///
  pub fn inc_counter(&mut self) -> Result<(), BitPackError> {
    self.p_bit += 1;
    if self.p_bit == 8 {
      self.p_bit = 0;
      self.p_byte += 1;
    }
    Ok(())
  }

  ///
  /// This operates together with `write_packed_bits`.  It only increments the
  /// `p_bit` value by 1, also incrementing `p_byte` where necessary.
  ///
  /// Note: The bit pointer must be byte aligned.
  ///
  /// ### Arguments
  ///
  /// * `n_bytes` - The number of bytes to increment.
  pub fn inc_counter_n_bytes(&mut self, n_bytes: usize) -> Result<(), BitPackError> {
    if self.p_bit != 0 {
      return Err(BitPackError::NotByteAligned);
    }
    self.p_byte += n_bytes;
    Ok(())
  }

  ///
  /// Move to the next byte, but only if we have the bit pointer is part the way through a byte.
  ///
  pub fn complete_byte(&mut self) -> Result<(), BitPackError> {
    if self.p_bit != 0 {
      self.p_byte += 1;
      self.p_bit = 0;
    }
    Ok(())
  }

  ///
  /// Align the packing to the next word, but only if we aren't already aligned.
  ///
  pub fn word_align(&mut self) -> Result<(), BitPackError> {
    if self.p_bit != 0 {
      self.p_byte += 1;
      self.p_bit = 0;
    }
    if self.p_byte % 2 == 1 {
      self.p_byte += 1;
    }
    Ok(())
  }

  ///
  /// Pack array value into the byte array.  Starting at `p_byte` position of the array and `p_bit` bit offset.
  ///
  /// ### Arguments
  ///
  /// * `value` - The bits that will be written.
  /// * `num_bits` - The number of bits in `value` that should be written.
  ///
  #[inline(always)]
  pub fn write_bits(&mut self, mut value: usize, num_bits: usize) -> Result<(), BitPackError> {
    let rem_bit = 8 - self.p_bit;
    let mask = (1 << num_bits) - 1;
    value &= mask;

    if num_bits == rem_bit {
      self.array[self.p_byte] |= value as u8;
      self.p_byte += 1;
      self.p_bit = 0;
    } else if num_bits < rem_bit {
      let shift_l = rem_bit - num_bits;
      self.array[self.p_byte] |= (value << shift_l) as u8;
      self.p_bit += num_bits;
    } else {
      let shift_r = num_bits - rem_bit;
      self.array[self.p_byte] |= (value >> shift_r) as u8;

      self.p_bit = 0;
      self.p_byte += 1;

      self.write_bits(value, shift_r)?;
    }
    Ok(())
  }

  ///
  /// This operates together with `write_packed_bits`.  It allows zero values to be written.  Although
  /// these are never actually written to the array, the offsets are just managed.
  ///
  /// ### Arguments
  ///
  /// * `num_zeros` - The number of zeros that should be written.
  ///
  #[inline(always)]
  pub fn write_packed_zeros(&mut self, num_zeros: usize) -> Result<(), BitPackError> {
    self.p_bit += num_zeros;
    while self.p_bit >= 8 {
      self.p_bit -= 8;
      self.p_byte += 1;
    }
    Ok(())
  }

  ///
  /// Returns the packed bits as an array.
  ///
  pub fn as_bytes(&self) -> Result<&[u8], BitPackError> {
    Ok(&self.array[0..self.p_byte])
  }
}

//
// ######                       ######
// #     # #     # ##### ###### #     # ######   ##   #####  ###### #####
// #     #  #   #    #   #      #     # #       #  #  #    # #      #    #
// ######    # #     #   #####  ######  #####  #    # #    # #####  #    #
// #     #    #      #   #      #   #   #      ###### #    # #      #####
// #     #    #      #   #      #    #  #      #    # #    # #      #   #
// ######     #      #   ###### #     # ###### #    # #####  ###### #    #
//

///
/// BitReader allows individual bits to be read from an array of bytes.
///
pub struct ByteReader<'a> {
  array: &'a [u8],
  p_byte: usize, // Byte pointer
}

impl<'a> ByteReader<'a> {
  pub fn new(array: &'a [u8]) -> ByteReader {
    ByteReader { array, p_byte: 0 }
  }

  pub fn reset(&mut self) {
    self.p_byte = 0;
  }

  pub fn set_pos(&mut self, p_byte: usize) {
    self.p_byte = p_byte;
  }

  pub fn get_pos(&self) -> usize {
    self.p_byte
  }

  pub fn find_le_u16(&mut self, word: u16) -> bool {
    if self.p_byte >= self.array.len() {
      return false;
    }

    let b0 = (word >> 8) as u8;
    let b1 = (word & 255) as u8;

    for i in self.p_byte..self.array.len() - 1 {
      let a0 = self.array[i];
      let a1 = self.array[i + 1];
      if a0 == b0 && a1 == b1 {
        return true;
      }
      self.p_byte += 1;
    }
    false
  }

  pub fn extract(&self, p_start: usize, p_end: usize) -> Result<Vec<u8>, BitPackError> {
    if p_start > self.array.len() || p_end > self.array.len() {
      Err(BitPackError::ArrayEndReached)
    } else {
      Ok(self.array[p_start..p_end].to_vec())
    }
  }

  ///
  /// Check if `buf` and ByteReader array at the current read position contain the
  /// same information.
  ///
  /// Note:
  ///
  /// ### Arguments
  /// * `buf` - The array where the bytes will be written to.
  ///
  #[inline(always)]
  pub fn eq(&self, buf: &[u8]) -> bool {
    let mut p = self.p_byte;
    for b in buf {
      if *b != self.array[p] {
        return false;
      };
      p += 1;
    }
    true
  }

  ///
  /// Get the number of bytes remaining in the ByteReader buffer.
  ///
  #[inline(always)]
  pub fn remaining_bytes(&self) -> Result<usize, BitPackError> {
    if self.p_byte > self.array.len() {
      Err(BitPackError::ArrayEndReached)
    } else {
      Ok(self.array.len() - self.p_byte)
    }
  }

  ///
  /// This operates together with `write_packed_bits`.  It increments the
  /// `p_byte` value by `n_bytes`.
  ///
  #[inline(always)]
  pub fn inc_counter(&mut self, n_bytes: usize) -> Result<(), BitPackError> {
    if self.p_byte + n_bytes >= self.array.len() {
      return Err(BitPackError::ArrayEndReached);
    }
    self.p_byte += n_bytes;

    Ok(())
  }

  ///
  /// This operates together with `write_packed_bits`.  It decrements the
  /// `p_byte` value by `n_bytes`.
  ///
  #[inline(always)]
  pub fn dec_counter(&mut self, n_bytes: usize) -> Result<(), BitPackError> {
    if n_bytes > self.p_byte {
      return Err(BitPackError::BoundaryReached);
    }
    self.p_byte -= n_bytes;

    Ok(())
  }

  ///
  /// Read `buf.len()` bytes and write them to buf.
  ///
  /// ### Arguments
  /// * `buf` - The array where the bytes will be written to.
  ///
  pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, BitPackError> {
    let bytes_written = if buf.len() > self.remaining_bytes()? {
      self.remaining_bytes()?
    } else {
      buf.len()
    };

    for (i, p_buf) in buf[..bytes_written].iter_mut().enumerate() {
      *p_buf = self.array[self.p_byte + i];
    }
    self.p_byte += bytes_written;

    Ok(bytes_written)
  }

  ///
  /// Read the next two bytes as big-endian u16.
  ///
  #[inline(always)]
  pub fn read_u8(&mut self) -> Result<u8, BitPackError> {
    let value = self.array[self.p_byte];
    self.p_byte += 1;
    Ok(value)
  }

  ///
  /// Read the next two bytes as big-endian u16.
  ///
  #[inline(always)]
  pub fn read_be_u16(&mut self) -> Result<u16, BitPackError> {
    let value = BigEndian::read_u16(&self.array[self.p_byte..]);
    self.p_byte += 2;
    Ok(value)
  }

  ///
  /// Read the next two bytes as big-endian i16.
  ///
  #[inline(always)]
  pub fn read_be_i16(&mut self) -> Result<i16, BitPackError> {
    let value = BigEndian::read_i16(&self.array[self.p_byte..]);
    self.p_byte += 2;
    Ok(value)
  }

  ///
  /// Read the next two bytes as little-endian i16.
  ///
  #[inline(always)]
  pub fn read_le_i16(&mut self) -> Result<i16, BitPackError> {
    let value = LittleEndian::read_i16(&self.array[self.p_byte..]);
    self.p_byte += 2;
    Ok(value)
  }

  pub fn crc16(&self, num_bytes: usize) -> Result<u16, BitPackError> {
    if self.p_byte + num_bytes > self.array.len() {
      Err(BitPackError::ArrayEndReached)
    } else {
      Ok(crc16(&self.array[self.p_byte..(self.p_byte + num_bytes)]))
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
  use crate::bitpack::BitPacker;

  #[test]
  fn test_write_packed_bits() {
    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 1;
    bp.write_bits(0x03, 2).unwrap();
    assert_eq!(1, bp.p_byte);
    assert_eq!(3, bp.p_bit);
    assert_eq!(&[0x00, 0x60, 0x00], bp.array);

    let inp_arr: &mut [u8] = &mut [0xff, 0x80, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 1;
    bp.write_bits(0x03, 2).unwrap();
    assert_eq!(1, bp.p_byte);
    assert_eq!(3, bp.p_bit);
    assert_eq!(&[0xff, 0xE0, 0x00], bp.array);

    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 5;
    bp.write_bits(0x1ff, 9).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(6, bp.p_bit);
    assert_eq!(&[0x00, 0x07, 0xfc], bp.array);

    let inp_arr: &mut [u8] = &mut [0xff, 0xfc, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 5;
    bp.write_bits(0x1ff, 9).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(6, bp.p_bit);
    assert_eq!(&[0xff, 0xff, 0xfc], bp.array);

    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 0;
    bp.p_bit = 6;
    bp.write_bits(0x1f27b, 17).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(7, bp.p_bit);
    assert_eq!(&[0x03, 0xe4, 0xf6], bp.array);

    let inp_arr: &mut [u8] = &mut [0xfe, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 0;
    bp.p_bit = 6;
    bp.write_bits(0x1f27b, 17).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(7, bp.p_bit);
    assert_eq!(&[0xff, 0xe4, 0xf6], bp.array);

    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 4;
    bp.write_bits(0x09, 4).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(0, bp.p_bit);
    assert_eq!(&[0x00, 0x09, 0x00], bp.array);

    let inp_arr: &mut [u8] = &mut [0xf0, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 0;
    bp.p_bit = 4;
    bp.write_bits(0xffffbe81, 16).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(4, bp.p_bit);
    assert_eq!(&[0xfb, 0xe8, 0x10], bp.array);

    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 1;
    bp.write_bits(0xfffffffc, 6).unwrap();
    assert_eq!(1, bp.p_byte);
    assert_eq!(7, bp.p_bit);
    assert_eq!(&[0x00, 0x78, 0x00], bp.array);

    let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
    let mut bp = BitPacker::new(inp_arr);
    bp.p_byte = 1;
    bp.p_bit = 2;
    bp.write_bits(0xfffffffc, 6).unwrap();
    assert_eq!(2, bp.p_byte);
    assert_eq!(0, bp.p_bit);
    assert_eq!(&[0x00, 0x3c, 0x00], bp.array);
  }
}
