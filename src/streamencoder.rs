use crate::bitpacker::{BitPacker, BitPackerState};
use crate::bytewriter::{ByteWriter, SeekFrom};
use crate::crc::{*};
use crate::encoder;
use crate::error::{Result, X3Error};
use crate::x3::{self};

/// Optional generic parameters `MAX_CHANNEL_COUNT` and `MAX_BLOCK_LENGTH` set
/// internal buffer sizes for reduced memory usage when the exact encoding
/// parameters are known at compile time
pub struct StreamEncoder<
    'a,
    W: ByteWriter,
    const MAX_CHANNEL_COUNT: usize = {x3::Parameters::MAX_CHANNEL_COUNT},
    const MAX_BLOCK_LENGTH: usize = {x3::Parameters::MAX_BLOCK_LENGTH},
> {
    /// output byte stream
    writer: &'a mut W,
    bitpacker_state: Option<BitPackerState>,
    frame_header_position: u64,
    filter_state: [i16; MAX_CHANNEL_COUNT],
    collected_sample_buffer: [[i16; MAX_BLOCK_LENGTH]; MAX_CHANNEL_COUNT],
    next_ch: usize,
    collected_sample_count: usize,
    block_count: usize,
    params: &'a x3::Parameters,
    sample_rate: u32,
}

impl<'a, W: ByteWriter, const CH: usize, const BL: usize> Drop for StreamEncoder<'a, W, CH, BL> {
    fn drop(&mut self) {
        let _ = self.encode_block();
        let _ = self.complete_frame();
    }
}

impl<'a, W: ByteWriter, const CH: usize, const BL: usize> StreamEncoder<'a, W, CH, BL> {
    //
    // Write <Archive Header> to the ByteWriter output.
    //
    fn create_archive_header (
        &mut self
    ) -> Result<()> {
    // <Archive Id>
    self.writer.write_all(x3::Archive::ID)?;

    // Make space for the header
    let frame_header_pos = self.writer.stream_position()?;
    self.writer.seek(SeekFrom::Current(x3::FrameHeader::LENGTH as i64))?;
    
    let mut sample_rate_str_buffer = itoa::Buffer::new();
    let sample_rate_str = sample_rate_str_buffer.format(self.sample_rate);

    let mut block_len_str_buffer = itoa::Buffer::new();
    let block_len_str = block_len_str_buffer.format(self.params.block_len);

    let mut code_str_buffer_0 = itoa::Buffer::new();
    let mut code_str_buffer_1 = itoa::Buffer::new();
    let mut code_str_buffer_2 = itoa::Buffer::new();
    let code_str = [
        code_str_buffer_0.format(self.params.codes[0]),
        code_str_buffer_1.format(self.params.codes[1]),
        code_str_buffer_2.format(self.params.codes[2]),
    ];

    let mut threshold_str_buffer_0 = itoa::Buffer::new();
    let mut threshold_str_buffer_1 = itoa::Buffer::new();
    let mut threshold_str_buffer_2 = itoa::Buffer::new();
    let threshold_str = [
        threshold_str_buffer_0.format(self.params.thresholds[0]),
        threshold_str_buffer_1.format(self.params.thresholds[1]),
        threshold_str_buffer_2.format(self.params.thresholds[2]),
    ];

    let xml: &str = &[
        // "<X3A>",
        // "<?xml version=\"1.0\" encoding=\"US-ASCII\" ?>",
        "<X3ARCH PROG=\"x3new.m\" VERSION=\"2.0\" />",
        "<CFG ID=\"0\" FTYPE=\"XML\" />",
        "<CFG ID=\"1\" FTYPE=\"WAV\">",
        "<FS UNIT=\"Hz\">",sample_rate_str,"</FS>",
        "<SUFFIX>wav</SUFFIX>",
        "<CODEC TYPE=\"X3\" VERS=\"2\">",
        "<BLKLEN>", block_len_str ,"</BLKLEN>",
        "<CODES N=\"4\">RICE", code_str[0], ",RICE", code_str[1], ",RICE", code_str[2], ",BFP</CODES>",
        "<FILTER>DIFF</FILTER>",
        "<NBITS>16</NBITS>",
        "<T N=\"3\">",threshold_str[0],",",threshold_str[1],",",threshold_str[2],"</T>",
        "</CODEC>",
        "</CFG>",
        // "</X3A>",
    ]
    .concat();
    let xml_bytes = xml.as_bytes();
    // <XML MetaData>
    let mut payload_len = xml_bytes.len();
    let mut payload_crc = crc16(xml_bytes);
    self.writer.write_all(xml_bytes)?;
    if payload_len % 2 == 1 {
        // Align to the nearest word
        self.writer.write_all([0u8])?;
        payload_len += 1;
        payload_crc = update_crc16(payload_crc, &0u8);
    }

    // <Frame Header>
    // Write the header details
    let return_position = self.writer.stream_position()?;
    self.writer.seek(SeekFrom::Start(frame_header_pos))?;
    let frame_header = encoder::write_frame_header(0, 0, 0, 0, payload_len, payload_crc);
    self.writer.write_all(frame_header)?;
    self.writer.seek(SeekFrom::Start(return_position))?;
    Ok(())
    }
        
    pub fn new(writer: &'a mut W, params: &'a x3::Parameters) -> Self {
        
        StreamEncoder{
            writer,
            bitpacker_state: None,
            frame_header_position: 0,
            filter_state: [0; CH],
            collected_sample_buffer : [[0; BL]; CH],
            next_ch: 0,
            collected_sample_count: 0,
            block_count: 0,
            params,
            sample_rate: 44100,
        }
    }

    fn encode_block(&mut self) -> Result<()>{
        if self.bitpacker_state.is_none(){
            return Ok(()) // At start of new frame nothing to flush
        }

        if self.next_ch != 0 {
            return Err(X3Error::EncodeStreamMismatchedChannelLengths);
        }

        for (fs, block) in self.filter_state.iter_mut().zip(self.collected_sample_buffer.iter())
        {
            let block = &block[..self.collected_sample_count];
            // construct diff block 
            let mut diff = [0i32; BL];
            for (i, sample) in block.iter().enumerate() {
                diff[i]  = i32::from(*sample) - i32::from(*fs);
                *fs = *sample;
            }
            let diff = &diff[..self.collected_sample_count];

            if let Some(bp_state) = &self.bitpacker_state {
                let mut bp = BitPacker::restore(self.writer, bp_state);
                encoder::x3_encode_block(block, &diff, &mut bp, self.params)?;
                self.bitpacker_state = Some(bp.store());
            } else {
                return Err(X3Error::EncodeStreamMissingBitpacker);
            }
        }
        Ok(())
    }

    fn complete_frame(&mut self) -> Result<()>{
        // finish header
        if let Some(bp_state) = &self.bitpacker_state {
            let (payload_len, payload_crc) = {
                let mut bp = BitPacker::restore(self.writer, bp_state);
                bp.word_align()?;
                (bp.len(), bp.crc())
            };
            self.bitpacker_state = None;

            // Write the header details
            let return_position = self.writer.stream_position()?;
            self.writer.seek(SeekFrom::Start(self.frame_header_position))?;
            // FIXME: Need to add the time
            let frame_header = encoder::write_frame_header(self.collected_sample_count + 1, 1, self.params.channel_count as u8, 0, payload_len, payload_crc);
            self.writer.write_all(frame_header)?;
            self.writer.seek(SeekFrom::Start(return_position))?;

            // reset stream state
            self.next_ch = 0;
            self.block_count = 0;
            self.collected_sample_count = 0;
        }
        Ok(())
    }

    pub fn close(mut self) -> Result<()> {
        self.encode_block()?;
        self.complete_frame()?;
        Ok(())
    }

    pub fn process_interleaved<'f, I>(&mut self, iter: impl IntoIterator<IntoIter = I>) -> Result<()>
    where 
        I: Iterator<Item = &'f i16>
    {
        let mut iter = iter.into_iter();
        
        // remain in loop as long as there are samples in the input iterator
        loop{
            /* NEW FRAME */
            if self.bitpacker_state.is_none() {
                // collect filter states
                while self.next_ch < self.params.channel_count  {
                    if let Some(fs) = iter.next() {
                        self.filter_state[self.next_ch] = *fs;
                        self.next_ch = self.next_ch + 1;
                    } else {
                        return Ok(()); // wait for more samples
                    }
                }
                self.next_ch = 0;

                // bookmark frame header
                self.frame_header_position = self.writer.stream_position()?;
                self.writer.seek(SeekFrom::Current(x3::FrameHeader::LENGTH as i64))?;
                
                
                let mut bp = BitPacker::new(self.writer);
                // write filter states for each channel
                for fs in self.filter_state {
                    bp.write_bits(fs as usize, 16)?;
                }
                self.bitpacker_state = Some(bp.store());
                
            }

            /* Collect block for each channel */
            // collect a block worth's of samples on all channels
            while  self.collected_sample_count != self.params.block_len {
                if let Some(sample) = iter.next() {
                    self.collected_sample_buffer[self.next_ch][self.collected_sample_count] = *sample;
                    self.next_ch += 1;
                    if self.next_ch == self.params.channel_count {
                        self.next_ch = 0;
                        self.collected_sample_count += 1;
                    }
                } else {
                    return Ok(()); // wait for more samples
                }
            }

            // encode sample block for each channel
            self.encode_block()?;
            self.block_count += 1;

            // check if frame is complete
            if self.block_count == self.params.blocks_per_frame {
                self.complete_frame()?;
            }
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
    use crate::streamencoder::StreamEncoder;
    use crate::bytewriter::{ByteWriter, SliceByteWriter};
    use crate::x3::{self, Parameters};

    const NUM_SAMPLES: usize = 0x0eff;


#[test]
  fn test_encode_frame_zeros() {
    let wav: &[i16] = &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let wl: usize = wav.len();
    let wlh = (wl >> 8) as u8;
    let wll = (wl & 0xff) as u8;
    let expected_x3_output: &[u8] = &[
      // Frame header
      b'x', b'3', // "x3"
      1, 1, // Source Id, Num Channels
      wlh, wll, // Num samples
      0, 6, // Num encoded bytes
      0, 0, 0, 0, 0, 0, 0, 0, // Time
      194, 242, // Header CRC
      205, 128, // Payload CRC
      // Frame payload
      0, 0, 127, 255, 248, 0,
    ];
    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2];
    let valid_len = {
      let writer = &mut SliceByteWriter::new(x3_output);
      let params = &Parameters::default();
      
      // make stream encoder
      let mut encoder : StreamEncoder<_, 1, {x3::Parameters::DEFAULT_BLOCK_LENGTH}> = StreamEncoder::new(writer, params);
      let mut wav_iter = wav.iter();
      let take_3 = wav_iter.by_ref().take(3);
        
      // stream some samples
      let _ = encoder.process_interleaved(take_3);

      // stream remaining samples
      let _ = encoder.process_interleaved(wav_iter);
        
      // close stream encoder
      let _ = encoder.close();
        
      writer.stream_position().unwrap() as usize
    }; // end writer scope

    assert_eq!(expected_x3_output, &x3_output[..valid_len]);
  }

}