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

// TODO: Can we get this from std::u32?
const BIT_LEN: usize = 32;

pub struct BitReader<'a> {
    array: &'a [u8],

    /// Byte pointer to the byte within the array
    idx: usize,

    /// Leading byte, such that we don't need to read the array the whole time
    leading_word: u32,

    /// The remaining number of bits in the word
    rem_bit: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(array: &'a [u8]) -> Self {
        Self {
            array,
            idx: 4,
            leading_word: BigEndian::read_u32(&array[0..]),
            rem_bit: BIT_LEN,
        }
    }

    /// Increment the bits, load a new byte if required.
    #[inline(always)]
    pub fn inc_bits(&mut self, n: usize) {
        debug_assert!(n < BIT_LEN);
        if n == 0 {
            return;
        }

        if n < self.rem_bit {
            self.leading_word <<= n;
            self.rem_bit -= n;
        } else if n == self.rem_bit {
            self.get_next();
        } else {
            //  n > self.rem_bit
            let rem = n - self.rem_bit;
            self.get_next();
            self.rem_bit = BIT_LEN - rem;
            self.leading_word <<= rem;
        }
    }

    ///
    /// Read the n number of bites in a packed bit array.
    ///
    /// ### Arguments
    ///
    /// * `num_bits` - The number of bits to read.
    ///
    /// ### Returns
    ///
    /// * The unsigned value returned.
    ///
    #[inline(always)]
    pub fn read_nbits(&mut self, n: usize) -> u32 {
        if n <= self.rem_bit {
            let result = self.leading_word >> (BIT_LEN - n);
            self.inc_bits(n);
            result
        } else {
            let rem = n - self.rem_bit;
            let mut result = self.leading_word >> (BIT_LEN - n);
            self.inc_bits(self.rem_bit);
            result |= self.leading_word >> (BIT_LEN - rem);
            self.inc_bits(rem);
            result
        }
    }

    ///
    /// Read the number of zeros in a packed bit array.  Loads a new byte if needed.
    ///
    /// ### Returns
    ///
    /// * the number of consectutive zeros found in the array.
    ///
    #[inline(always)]
    pub fn count_zero_bits(&mut self) -> usize {
        let mut count = self.leading_word.leading_zeros() as usize;
        if count > self.rem_bit {
            count = match self.peek_next() {
                Some((word, _)) => self.rem_bit + word.leading_zeros() as usize,
                None => self.rem_bit,
            };
        }
        self.inc_bits(count);
        count
    }

    ///
    /// Get the next byte.
    ///
    /// ### Returns
    ///
    /// * the next byte
    ///
    #[inline(always)]
    fn get_next(&mut self) -> u32 {
        match self.peek_next() {
            Some((word, diff_idx)) => {
                self.leading_word = word;
                self.idx += diff_idx;
                self.rem_bit = diff_idx * 8;
                word
            }
            None => {
                self.leading_word = 0;
                self.rem_bit = 0;
                0
            }
        }
    }

    ///
    /// Peek at the next byte, without incrementing our pointer.
    ///
    #[inline(always)]
    fn peek_next(&self) -> Option<(u32, usize)> {
        if self.idx >= self.array.len() {
            None
        } else if self.array.len() - self.idx >= 4 {
            let word = BigEndian::read_u32(&self.array[self.idx..]);
            Some((word, 4))
        } else {
            // We are at the end of the array
            let remaining_idx = self.array.len() - self.idx;
            let mut word = 0u32;
            if remaining_idx >= 1 {
                word |= (self.array[self.idx] as u32) << (3 * 8);
            }
            if remaining_idx >= 2 {
                word |= (self.array[self.idx + 1] as u32) << (2 * 8);
            }
            if remaining_idx == 3 {
                word |= (self.array[self.idx + 2] as u32) << (1 * 8);
            }
            Some((word, remaining_idx))
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
    use crate::bitreader::BitReader;

    #[test]
    fn test_bitreader_init() {
        let inp_arr: &mut [u8] = &mut [0x00, 0x0f, 0xf0, 0x00];
        let br = BitReader::new(inp_arr);

        assert_eq!(32, br.rem_bit);
        assert_eq!(0x000ff000, br.leading_word);
    }

    #[test]
    fn test_count_zero_bits() {
        let inp_arr: &mut [u8] = &mut [0x00, 0x0f, 0xf0, 0x00];
        let mut br = BitReader::new(inp_arr);

        // Read first 12 zero
        {
            let zeros = br.count_zero_bits();
            assert_eq!(12, zeros);
            assert_eq!(20, br.rem_bit);
            assert_eq!(0xff000000, br.leading_word);
        }

        // Next there are no zeros
        {
            let zeros = br.count_zero_bits();
            assert_eq!(0, zeros);
            assert_eq!(20, br.rem_bit);
            assert_eq!(0xff000000, br.leading_word);
        }

        // Skip some bits
        {
            let byte = br.read_nbits(7);
            assert_eq!(0x7f, byte);
            assert_eq!(13, br.rem_bit);
            assert_eq!(0x80000000, br.leading_word);
        }
        {
            let byte = br.read_nbits(1);
            assert_eq!(0x01, byte);
            assert_eq!(12, br.rem_bit);
            assert_eq!(0x00000000, br.leading_word);
        }

        // Read to the end
        {
            let zeros = br.count_zero_bits();
            assert_eq!(12, zeros);

            assert_eq!(0, br.rem_bit);
            assert_eq!(0x00000000, br.leading_word);
        }
    }

    #[test]
    fn test_bitreader_long_array() {
        let inp_arr: &mut [u8] = &mut [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01];

        let mut br = BitReader::new(inp_arr);

        // Basic load
        {
            assert_eq!(32, br.rem_bit);
            assert_eq!(0b00000001001000110100010101100111, br.leading_word);
        }

        // Load next bytes
        {
            let byte = br.read_nbits(20);
            assert_eq!(0b00000001001000110100, byte);
            assert_eq!(12, br.rem_bit);
            assert_eq!(0b010101100111 << 20, br.leading_word);
        }

        // Load next bits
        {
            let bit = br.read_nbits(1);
            assert_eq!(0b0, bit);
            assert_eq!(0b10101100111000000000000000000000, br.leading_word);

            let bit = br.read_nbits(1);
            assert_eq!(0b1, bit);
            assert_eq!(0b01011001110000000000000000000000, br.leading_word);
        }
        {
            let bits = br.read_nbits(5);
            assert_eq!(0b01011, bits);
            assert_eq!(0b00111000000000000000000000000000, br.leading_word);

            let bits = br.read_nbits(6);
            assert_eq!(0b001111, bits);
            assert_eq!(0b00010011010101111001101111011110, br.leading_word);

            let bits = br.read_nbits(31);
            assert_eq!(0x09abcdef, bits);
            assert_eq!(0x01000000, br.leading_word);

            let bits = br.read_nbits(8);
            assert_eq!(0x01, bits);
            assert_eq!(0, br.leading_word);
        }
    }
}
