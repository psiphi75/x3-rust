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
#![no_std]

#[cfg(feature = "std")]
extern crate std;

#[cfg(any(feature = "alloc", feature = "std"))]
extern crate alloc;

extern crate byteorder;
extern crate hound;

pub mod bitpacker;
pub mod bitreader;
pub mod bytereader;
pub mod bytewriter;
pub mod crc;
#[cfg(feature = "std")]
pub mod decodefile;
pub mod decoder;
#[cfg(feature = "std")]
pub mod encodefile;
pub mod encoder;
pub mod error;
pub mod x3;
mod utils;
