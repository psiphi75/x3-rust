/**************************************************************************
 *                                                                        *
 * Rust implementation of the X3 lossless audio compression protocol.     *
 *                                                                        *
 * Copyright (C) 2026 Simon M. Werner <simonwerner@gmail.com>             *
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

use std::vec;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use x3::bytewriter::SliceByteWriter;
use x3::decoder;
use x3::encoder::encode_frame;

const NUM_SAMPLES: usize = 16 * 1024;

fn bench_thrpt(c: &mut Criterion, label: &str, bytes: usize, f: impl Fn() + 'static) {
  let mut group = c.benchmark_group(label);
  group.throughput(Throughput::Bytes(bytes as u64));
  group.bench_function("throughput", |b| {
    b.iter(|| {
      f();
    })
  });
  group.finish();
}

fn create_wav_buffer(scaler: f32) -> Vec<i16> {
  let mut wav: Vec<i16> = Vec::with_capacity(NUM_SAMPLES);
  for i in 0..NUM_SAMPLES {
    let t = std::f32::consts::PI * i as f32 / 180.0;
    let sample = (t.sin()) * scaler * i16::max_value() as f32;
    wav.push(sample as i16);
  }
  wav
}

fn encode(c: &mut Criterion, scale: f32) {
  // Create the WAV buffer and the label for the benchmark
  let wav = create_wav_buffer(scale);
  let label = format!("encode_frame - {}", scale);

  // Benchmark the time and throughput of encoding
  bench_thrpt(c, &label, wav.len() * 2, move || {
    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2];
    let writer = &mut SliceByteWriter::new(x3_output);
    let params = &x3::x3::Parameters::default();
    let stats: &mut [usize; 6] = &mut [0; 6];
    encode_frame(&wav, writer, params, stats).unwrap();
  });

}

fn decode(c: &mut Criterion, scale: f32) {
  // Create the WAV buffer and the label for the benchmark
  let wav = create_wav_buffer(scale);
  let label = format!("decode_frame - {}", scale);
  let mut x3_output = vec![0u8; NUM_SAMPLES * 2];

  // Encode the original wav data
  let params = x3::x3::Parameters::default();
  let writer = &mut SliceByteWriter::new(&mut x3_output);
  let stats: &mut [usize; 6] = &mut [0; 6];
  encode_frame(&wav, writer, &params, stats).unwrap();

  // Now decode the data we just encoded
  let frame_header = decoder::read_frame_header(&x3_output).unwrap();
  let samples = frame_header.samples as usize;

  // Benchmark the time and throughput of decoding
  bench_thrpt(c, &label, NUM_SAMPLES * 2, move || {
    let mut wav_output = [0i16; NUM_SAMPLES];
    decoder::decode_frame(
      &x3_output[x3::x3::FrameHeader::LENGTH..],
      &mut wav_output,
      &params,
      samples,
    )
    .unwrap();
  });
}

fn criterion_benchmark(c: &mut Criterion) {
  // Benchmark encoding with different scalers to get different types of entropy.
  encode(c, 0.0);   // Targets Rice-0 encoding
  encode(c, 0.03);  // Targets Rice-0, Rice-1, and Rice-3 encoding
  encode(c, 0.5);   // Targets BPF encoding

  decode(c, 0.0);   // Targets Rice-0 encoding
  decode(c, 0.03);  // Targets Rice-0, Rice-1, and Rice-3 encoding
  decode(c, 0.5);   // Targets BPF encoding
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
