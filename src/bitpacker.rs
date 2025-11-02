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

//
//      ######          ######
//      #     # # ##### #     #   ##    ####  #    # ###### #####
//      #     # #   #   #     #  #  #  #    # #   #  #      #    #
//      ######  #   #   ######  #    # #      ####   #####  #    #
//      #     # #   #   #       ###### #      #  #   #      #####
//      #     # #   #   #       #    # #    # #   #  #      #   #
//      ######  #   #   #       #    #  ####  #    # ###### #    #
//

#[derive(Debug)]
pub enum BitPackError {
    NotByteAligned,      // The bytes are not aligned.
    BoundaryReached,     // The soft boundary has been reached.
    ArrayEndReached,     // The end of the array has been reached.
    ExceededBitBoundary, // More bits were read than we expected
}

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
    pub fn new(array: &'a mut [u8]) -> BitPacker<'a> {
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
    pub fn bookmark_get_from(&self) -> &[u8] {
        &self.array[self.bm_p_byte..self.p_byte]
    }

    ///
    /// Get number of bytes from the bookmark to the current pointer.
    ///
    pub fn bookmark_get_offset(&self) -> usize {
        let offset = if self.p_bit == 0 { 0 } else { 1 };
        self.p_byte - self.bm_p_byte + offset
    }

    ///
    /// Write the array from the bookmark onwards, until array is
    /// exhausted.
    ///
    pub fn bookmark_write(&mut self, array: &[u8]) {
        for (i, value) in array.iter().enumerate() {
            self.array[self.bm_p_byte + i] = *value;
        }
    }

    ///
    /// Standard write an array
    ///
    pub fn write_bytes(&mut self, array: &[u8]) {
        for value in array {
            self.array[self.p_byte] = *value;
            self.p_byte += 1;
        }
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
    /// Align the packing to the next word, but only if we aren't already aligned.
    ///
    pub fn word_align(&mut self) {
        if self.p_bit != 0 {
            self.p_byte += 1;
            self.p_bit = 0;
        }
        if self.p_byte % 2 == 1 {
            self.p_byte += 1;
        }
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
    pub fn write_bits(&mut self, mut value: usize, num_bits: usize) {
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

            self.write_bits(value, shift_r);
        }
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
    pub fn write_packed_zeros(&mut self, num_zeros: usize) {
        self.p_bit += num_zeros;
        while self.p_bit >= 8 {
            self.p_bit -= 8;
            self.p_byte += 1;
        }
    }

    ///
    /// Returns the packed bits as an array.
    ///
    pub fn as_bytes(&self) -> &[u8] {
        &self.array[0..self.p_byte]
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

    #[test]
    fn test_write_packed_bits() {
        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 1;
        bp.write_bits(0x03, 2);
        assert_eq!(1, bp.p_byte);
        assert_eq!(3, bp.p_bit);
        assert_eq!(&[0x00, 0x60, 0x00], bp.array);

        let inp_arr: &mut [u8] = &mut [0xff, 0x80, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 1;
        bp.write_bits(0x03, 2);
        assert_eq!(1, bp.p_byte);
        assert_eq!(3, bp.p_bit);
        assert_eq!(&[0xff, 0xE0, 0x00], bp.array);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 5;
        bp.write_bits(0x1ff, 9);
        assert_eq!(2, bp.p_byte);
        assert_eq!(6, bp.p_bit);
        assert_eq!(&[0x00, 0x07, 0xfc], bp.array);

        let inp_arr: &mut [u8] = &mut [0xff, 0xfc, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 5;
        bp.write_bits(0x1ff, 9);
        assert_eq!(2, bp.p_byte);
        assert_eq!(6, bp.p_bit);
        assert_eq!(&[0xff, 0xff, 0xfc], bp.array);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 0;
        bp.p_bit = 6;
        bp.write_bits(0x1f27b, 17);
        assert_eq!(2, bp.p_byte);
        assert_eq!(7, bp.p_bit);
        assert_eq!(&[0x03, 0xe4, 0xf6], bp.array);

        let inp_arr: &mut [u8] = &mut [0xfe, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 0;
        bp.p_bit = 6;
        bp.write_bits(0x1f27b, 17);
        assert_eq!(2, bp.p_byte);
        assert_eq!(7, bp.p_bit);
        assert_eq!(&[0xff, 0xe4, 0xf6], bp.array);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 4;
        bp.write_bits(0x09, 4);
        assert_eq!(2, bp.p_byte);
        assert_eq!(0, bp.p_bit);
        assert_eq!(&[0x00, 0x09, 0x00], bp.array);

        let inp_arr: &mut [u8] = &mut [0xf0, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 0;
        bp.p_bit = 4;
        bp.write_bits(0xffffbe81, 16);
        assert_eq!(2, bp.p_byte);
        assert_eq!(4, bp.p_bit);
        assert_eq!(&[0xfb, 0xe8, 0x10], bp.array);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 1;
        bp.write_bits(0xfffffffc, 6);
        assert_eq!(1, bp.p_byte);
        assert_eq!(7, bp.p_bit);
        assert_eq!(&[0x00, 0x78, 0x00], bp.array);

        let inp_arr: &mut [u8] = &mut [0x00, 0x00, 0x00];
        let mut bp = BitPacker::new(inp_arr);
        bp.p_byte = 1;
        bp.p_bit = 2;
        bp.write_bits(0xfffffffc, 6);
        assert_eq!(2, bp.p_byte);
        assert_eq!(0, bp.p_bit);
        assert_eq!(&[0x00, 0x3c, 0x00], bp.array);
    }
}
