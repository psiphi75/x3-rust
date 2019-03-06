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
///
/// This is just a test helper, it outputs the wav data a string
///
// externs
extern crate hound;

use clap::{App, Arg};

fn main() {
  let matches = App::new("wav_to_str")
    .version("0.1.0")
    .author("Simon Werner <simonwerner@gmail.com>")
    .about("wav_to_str - output a wav a string.")
    .arg(
      Arg::with_name("wav")
        .short("w")
        .long("wav")
        .value_name("FILE")
        .help("The input file, a .wavfile")
        .required(true)
        .takes_value(true),
    )
    .get_matches();

  let wav_filename = matches.value_of("wav").unwrap();
  let mut reader = hound::WavReader::open(wav_filename).unwrap();

  println!("bits_per_sample: {}", reader.spec().bits_per_sample);
  println!("channels: {}", reader.spec().channels);
  println!("sample_rate: {}", reader.spec().sample_rate);

  let samples = reader.samples::<i16>().map(|x| x.unwrap()).collect::<Vec<i16>>();
  let mut n = 0;
  for sample in samples {
    if n == 16 {
      println!("");
      n = 0;
    }
    n += 1;
    print!("{} ", sample);
  }
  println!("");
}
