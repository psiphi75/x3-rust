/**************************************************************************
 *                                                                        *
 * Rust implementation of the X3 lossless audio compression protocol.     *
 *                                                                        *
 * Copyright (C) 2020 Simon M. Werner <simonwerner@gmail.com>             *
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

use crate::crc::update_crc16;
//
//      ######          ######
//      #     # # ##### #     #   ##    ####  #    # ###### #####
//      #     # #   #   #     #  #  #  #    # #   #  #      #    #
//      ######  #   #   ######  #    # #      ####   #####  #    #
//      #     # #   #   #       ###### #      #  #   #      #####
//      #     # #   #   #       #    # #    # #   #  #      #   #
//      ######  #   #   #       #    #  ####  #    # ###### #    #
//
use crate::error::{Result, X3Error};
use crate::bytewriter::{ByteWriter, SeekFrom};

#[derive(Debug)]
pub enum BitPackError {
    NotByteAligned,      // The bytes are not aligned.
    BoundaryReached,     // The soft boundary has been reached.
    ArrayEndReached,     // The end of the array has been reached.
    ExceededBitBoundary, // More bits were read than we expected
    NoBookmarkSet,
}

///
/// BitPacker allows individual bits to be written to an array of bytes.
///
pub struct BitPacker<'a, W: ByteWriter> {
    writer: &'a mut W,
    // Bit pointer
    scratch_byte: u8,
    p_bit: usize,
    // Len and CRC
    byte_len: usize,
    crc: u16,
}

impl<'a, W: ByteWriter> Drop for BitPacker<'a, W> {
    fn drop(&mut self) {
        if self.p_bit != 0 {
            self.flush().unwrap();
        }
    }
}

impl<'a, W: ByteWriter> BitPacker<'a, W> {
    pub fn new(writer: &'a mut W) -> BitPacker<'a, W> {
        BitPacker {
            writer,
            scratch_byte: 0,
            p_bit: 0,
            byte_len: 0,
            crc: 0xffff,
        }
    }

    pub fn crc(&self) -> u16 {
        return self.crc;
    }

    fn flush(&mut self) -> Result<()> {
        self.crc = update_crc16(self.crc, &self.scratch_byte);
        self.byte_len += 1;
        self.writer.write_all([self.scratch_byte])?;
        self.scratch_byte = 0;
        self.p_bit = 0;
        Ok(())
    }

    pub fn len(&self) -> usize {
        return self.byte_len;
    }

    ///
    /// Standard write an array
    ///
    pub fn write_bytes(&mut self, array: &[u8]) -> Result<()> {
        self.byte_len += array.len();
        for d in array {
            self.crc = update_crc16(self.crc, d);
        }
        self.writer.write_all(array)?;
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
    pub fn inc_counter_n_bytes(&mut self, n_bytes: usize) -> Result<()> {
        if self.p_bit != 0 {
            return Err(X3Error::BitPack(BitPackError::NotByteAligned));
        }
        self.writer.seek(SeekFrom::Current(n_bytes as i64))?;
        Ok(())
    }

    ///
    /// Align the packing to the next word, but only if we aren't already aligned.
    ///
    pub fn word_align(&mut self) -> Result<()> {
        if self.p_bit != 0 {
            self.flush()?;
        }
        while 0 != (self.writer.stream_position()? % 2) {
            self.flush()?;
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
    pub fn write_bits(&mut self, mut value: usize, num_bits: usize)-> Result<()> {
        let rem_bit = 8 - self.p_bit;
        let mask = (1 << num_bits) - 1;
        value &= mask;

        if num_bits == rem_bit {
            self.scratch_byte |= value as u8;
            self.flush()?;
        } else if num_bits < rem_bit {
            let shift_l = rem_bit - num_bits;
            self.scratch_byte |= (value << shift_l) as u8;
            self.p_bit += num_bits;
        } else {
            let shift_r = num_bits - rem_bit;
            self.scratch_byte |= (value >> shift_r) as u8;
            self.flush()?;

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
    pub fn write_packed_zeros(&mut self, num_zeros: usize) -> Result<()> {
        self.write_bits(0, num_zeros)
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
    use crate::bytewriter::SliceByteWriter;

    #[test]
    fn test_write_packed_bits() {
        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0x0,9);
            let _ = bp.write_bits(0x3, 2);
        }
        assert_eq!(&[0x00, 0x60, 0x00], inp_arr);

        let inp_arr: &mut [u8] = &mut [0xff, 0x80, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0x1ff,9);
            let _ = bp.write_bits(0x3, 2);
        }
        assert_eq!(&[0xff, 0xE0, 0x00], inp_arr);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0,13);
            let _ = bp.write_bits(0x1ff, 9);
        }
        assert_eq!(&[0x00, 0x07, 0xfc], inp_arr);


        let inp_arr: &mut [u8] = &mut [0xff, 0xfc, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0x1fff,13);
            let _ = bp.write_bits(0x1ff, 9);
        }
        assert_eq!(&[0xff, 0xff, 0xfc], inp_arr);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0,6);
            let _ = bp.write_bits(0x1f27b, 17);
        }
        assert_eq!(&[0x03, 0xe4, 0xf6], inp_arr);

        let inp_arr: &mut [u8] = &mut [0xfe, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0x3f,6);
            let _ = bp.write_bits(0x1f27b, 17);
        }
        assert_eq!(&[0xff, 0xe4, 0xf6], inp_arr);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0,12);
            let _ = bp.write_bits(0x9, 4);
        }
        assert_eq!(&[0x00, 0x09, 0x00], inp_arr);

        let inp_arr: &mut [u8] = &mut [0xf0, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0xf,4);
            let _ = bp.write_bits(0xffffbe81, 16);
        }
        assert_eq!(&[0xfb, 0xe8, 0x10], inp_arr);


        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0,9);
            let _ = bp.write_bits(0xfffffffc, 6);
        }
        assert_eq!(&[0x00, 0x78, 0x00], inp_arr);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        {
            let writer = &mut SliceByteWriter::new(inp_arr);
            let mut bp = BitPacker::new(writer);
            let _ = bp.write_bits(0,10);
            let _ = bp.write_bits(0xfffffffc, 6);
        }
        assert_eq!(&[0x00, 0x3c, 0x00], inp_arr);
    }
}
