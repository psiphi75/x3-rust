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
use crate::bitpacker::BitPackError;
use crate::byteorder::{BigEndian, ByteOrder, LittleEndian};
use crate::crc::crc16;

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
