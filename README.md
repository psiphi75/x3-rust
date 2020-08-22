# X3 Lossless Audio Compression for Rust

X3 is a simple and effective lossless audio compressor for low entropy sound. It is based on
[Shorten](<https://en.wikipedia.org/wiki/Shorten_(file_format)>) and has some of the features
of [FLAC](https://xiph.org/flac/) but is designed specifically for use in underwater sound
recording tags and buoys. It is much faster than FLAC, but does not acheive the compression
ratio. The name comes from the compression factor that it usually achieves, i.e., 3 times.
The algorithm is described in a paper in the [Journal of the Acoustical Society of
America 133:1387-1398, 2013](http://link.aip.org/link/?JAS/133/1387).

This repository is a Rust port of the Matlab code from the
[original authors](https://www.soundtags.org/dtags/audio_compression/).

## Code usage

### Encode and decode .wav <-> .x3a

```rust

use x3::encodefile::wav_to_x3a;
use x3::decodefile::x3a_to_wav;

// Convert .wav to .x3a
wav_to_x3a("/path/to/input_file.x3a", "/path/to/output_file.wav").unwrap();

// Convert .x3a to .wav
x3a_to_wav("/path/to/input_file.wav", "/path/to/output_file.x3a").unwrap();

```

### Encode an array of wav data

```rust

  let wav: Vec<i16> = /* you need to add your wav data */;

  // Can only handle signed 16 bit data with one channel.
  let params = x3::Parameters::default();
  let sample_rate = 44100;
  let num_samples = wav.len();

  // Create the channel data
  let first_channel = x3::Channel::new(0, &wav, sample_rate, params);

  // Create the output data
  let x3_len = num_samples * 2;
  let mut x3_out = vec![0u8; x3_len];
  let bp = &mut BitPacker::new(&mut x3_out); // Packer where x3 compressed data is stored.

  encoder::encode(&[&first_channel], bp).unwrap();

  // Get the bytes
  let x3_bytes = bp.as_bytes().unwrap();

```

## Comand line usage

Building the package will create the `x3` binary executable. You can convert files
to/from x3a/wav.

Example:

```sh

# Convert from x3a to wav
./x3 --input /path/to/file.x3a --output /path/to/file.wav

# Convert from wav to x3a
./x3 --input /path/to/file.wav --output /path/to/file.x3a
```

## TODO

The following items need to be worked on:

- Inform user if there were issues with decoding frames.
- Error handling could be better.
- Add multiple channel capability - currently we can only handle mono.
- Explicity use a `#![no_std]` option for embedded devices.
- Peformance, it is currently slower than the flac encoder.
- Not very memory efficient for de/encoding files. It will allocate all the memory upfront.

## License

Rust implementation of the X3 lossless audio compression protocol.

Copyright (C) 2019 Simon M. Werner <simonwerner@gmail.com>

This program is free software; you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation; either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program; if not, see <https://www.gnu.org/licenses>.
