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

extern crate clap;
extern crate x3;

use clap::{App, Arg};

#[derive(PartialEq, Eq)]
enum AudioFiles {
  X3a,   // .x3a
  Wav,   // .wav
  X3Bin, // .bin
}

fn get_filetype(filename: &str) -> AudioFiles {
  if filename.ends_with(".x3a") {
    return AudioFiles::X3a;
  }
  if filename.ends_with(".wav") {
    return AudioFiles::Wav;
  }
  if filename.ends_with(".bin") {
    return AudioFiles::X3Bin;
  }
  panic!(
    "Invalid audio file, expecting a '.wav', '.bin' or '.x3a' file: {}",
    filename
  );
}

#[tokio::main]
async fn main() {
  let matches = App::new("x3")
    .version("0.1.0")
    .author("Simon Werner <simonwerner@gmail.com>")
    .about("x3 - efficient lossless compression for low entropy audio wav files.")
    .arg(
      Arg::with_name("input")
        .short("i")
        .long("input")
        .value_name("FILE")
        .help("The input file, a .wav or .x3a file")
        .required(true)
        .takes_value(true),
    )
    .arg(
      Arg::with_name("output")
        .short("o")
        .long("output")
        .value_name("FILE")
        .help("The output file, a .wav or .x3a file")
        .required(true)
        .takes_value(true),
    )
    .get_matches();

  let in_file = matches.value_of("input").unwrap();
  let out_file = matches.value_of("output").unwrap();

  let in_type = get_filetype(in_file);
  let out_type = get_filetype(out_file);

  if in_type == out_type {
    panic!("Input must be different file type than output.");
  }

  match in_type {
    AudioFiles::Wav => (), //FIXME: x3::encodefile::wav_to_x3a(in_file, out_file).await.unwrap(),
    AudioFiles::X3a => x3::decodefile::x3a_to_wav(in_file, out_file).await.unwrap(),
    AudioFiles::X3Bin => {
      assert!(out_type == AudioFiles::Wav);
      x3::decodefile::x3bin_to_wav(in_file, out_file).unwrap();
    }
  };
}
