use crate::bitpacker::{BitPacker, BitPackerState};
use crate::bytewriter::{ByteWriter, SeekFrom};
use crate::crc::{*};
use crate::encoder;
use crate::error::{Result, X3Error};
use crate::x3::{self};

/// 
/// Optional generic parameters `MAX_CHANNEL_COUNT` and `MAX_BLOCK_LENGTH` set
/// internal buffer sizes for reduced memory usage when the exact encoding
/// parameters are known at compile time
/// 
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
        if self.collected_sample_count != 0 {
            let _ = self.encode_block();
        }
        let _ = self.complete_frame();
    }
}

impl<'a, W: ByteWriter, const CH: usize, const BL: usize> StreamEncoder<'a, W, CH, BL> {
    ///
    /// Write <Archive Header> to the x3 stream encoder's ByteWriter output.
    ///
    pub fn create_archive_header (
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

    let xml_parts: &[&str] = &[
        // "<X3A>",
        // "<?xml version=\"1.0\" encoding=\"US-ASCII\" ?>",
        "<X3ARCH PROG=\"x3::streamencoder.rs\" VERSION=\"1.0\" />",
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
    ];

    // <XML MetaData>
    let mut payload_len = 0;
    let mut payload_crc = crc16(&[]);
    for part in xml_parts {
        let xml_bytes = part.as_bytes();
        payload_len += xml_bytes.len();
        for byte in xml_bytes {
            payload_crc = update_crc16(payload_crc,byte);
        }
        self.writer.write_all(xml_bytes)?;
    }

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

    ///
    /// Create an X3 stream encoder based on input params
    ///
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
                encoder::x3_encode_block(block, diff, &mut bp, self.params)?;
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
                let result = (bp.len(), bp.crc());
                self.bitpacker_state = Some(bp.store());
                result
            };
            self.bitpacker_state = None;

            // Write the header details
            let return_position = self.writer.stream_position()?;
            self.writer.seek(SeekFrom::Start(self.frame_header_position))?;
            // FIXME: Need to add the time
            let frame_header = encoder::write_frame_header(self.block_count*self.params.block_len + self.collected_sample_count + 1, 1, self.params.channel_count as u8, 0, payload_len, payload_crc);
            self.writer.write_all(frame_header)?;
            self.writer.seek(SeekFrom::Start(return_position))?;

            // reset stream state
            self.next_ch = 0;
            self.block_count = 0;
            self.collected_sample_count = 0;
        }
        Ok(())
    }

    ///
    /// Closes the current X3 stream encoder flushing any partially encoded blocks 
    /// and completing the current frame header
    /// 
    pub fn close(mut self) -> Result<()> {
        if self.collected_sample_count != 0 {
            self.encode_block()?;
        }
        self.complete_frame()?;
        Ok(())
    }

    ///
    /// Passes a sample iterator into the X3 stream encoder to be encoded
    /// 
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
                        self.next_ch += 1;
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
            // collect a block's worth of samples on all channels
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
            self.collected_sample_count = 0;

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
  fn test_encode_frame() {
    let wav: &[i16] = &[
      -3461, -3452, -3441, -3456, -3462, -3453, -3461, -3461, -3449, -3457, -3463, -3460, -3454, -3450, -3449, -3452,
      -3450, -3449, -3463, -3462, -3453, -3458, -3448, -3447, -3461, -3462, -3452, -3452, -3456, -3459, -3456, -3449,
      -3444, -3451, -3455, -3449, -3453, -3456, -3455, -3458, -3457, -3450, -3453, -3458, -3456, -3458, -3461, -3451,
      -3447, -3450, -3461, -3459, -3450, -3450, -3453, -3464, -3463, -3455, -3452, -3457, -3453, -3453, -3453, -3445,
      -3449, -3450, -3450, -3451, -3452, -3453, -3455, -3455, -3457, -3455, -3452, -3460, -3458, -3463, -3454, -3453,
      -3458, -3461, -3470, -3464, -3447, -3449, -3456, -3462, -3457, -3449, -3455, -3456, -3448, -3444, -3449, -3442,
      -3442, -3459, -3462, -3456, -3459, -3454, -3454, -3456, -3454, -3452, -3443, -3456, -3454, -3447, -3453, -3457,
      -3460, -3456, -3447, -3451, -3459, -3460, -3462, -3459, -3444, -3443, -3445, -3450, -3459, -3451, -3443, -3450,
      -3452, -3459, -3457, -3456, -3452, -3461, -3467, -3457, -3452, -3449, -3456, -3460, -3454, -3455, -3448, -3439,
      -3445, -3454, -3450, -3440, -3457, -3472, -3458, -3444, -3451, -3458, -3463, -3460, -3456, -3467, -3463, -3450,
      -3451, -3449, -3445, -3442, -3445, -3453, -3453, -3442, -3454, -3455, -3460, -3458, -3462, -3479, -3453, -3445,
      -3456, -3458, -3442, -3442, -3445, -3441, -3448, -3449, -3443, -3453, -3452, -3450, -3468, -3467, -3451, -3458,
      -3462, -3453, -3459, -3468, -3453, -3452, -3464, -3453, -3454, -3462, -3457, -3452, -3452, -3455, -3463, -3478,
      -3469, -3464, -3472, -3461, -3465, -3461, -3460, -3455, -3456, -3454, -3449, -3455, -3460, -3464, -3456, -3462,
      -3465, -3458, -3461, -3475, -3477, -3479, -3471, -3470, -3466, -3455, -3457, -3463, -3454, -3442, -3449, -3455,
      -3453, -3454, -3456, -3465, -3466, -3460, -3454, -3459, -3460, -3460, -3464, -3457, -3458, -3459, -3459, -3467,
      -3468, -3460, -3455, -3466, -3467, -3471, -3466, -3463, -3463, -3465, -3464, -3456, -3450, -3448, -3449, -3456,
      -3462, -3456, -3462, -3461, -3463, -3468, -3462, -3464, -3462, -3460, -3470, -3467, -3458, -3457, -3465, -3469,
      -3465, -3461, -3456, -3460, -3459, -3458, -3461, -3463, -3461, -3462, -3465, -3465, -3461, -3462, -3459, -3464,
      -3458, -3461, -3464, -3460, -3474, -3470, -3473, -3473, -3470, -3465, -3459, -3460, -3464, -3469, -3457, -3462,
      -3468, -3458, -3473, -3474, -3463, -3466, -3464, -3472, -3465, -3453, -3471, -3460, -3470, -3477, -3471, -3466,
      -3462, -3462, -3468, -3465, -3459, -3469, -3466, -3465, -3466, -3473, -3468, -3465, -3469, -3465, -3460, -3455,
      -3466, -3468, -3460, -3461, -3453, -3460, -3470, -3475, -3472, -3474, -3467, -3469, -3478, -3469, -3469, -3474,
      -3460, -3454, -3462, -3458, -3461, -3467, -3467, -3460, -3470, -3471, -3477, -3472, -3468, -3470, -3466, -3468,
      -3461, -3461, -3467, -3464, -3470, -3479, -3471, -3477, -3474, -3472, -3476, -3478, -3470, -3473, -3475, -3466,
      -3461, -3463, -3468, -3466, -3463, -3465, -3470, -3468, -3466, -3465, -3472, -3479, -3476, -3474, -3475, -3466,
      -3458, -3463, -3464, -3465, -3466, -3468, -3468, -3468, -3465, -3461, -3470, -3466, -3460, -3470, -3468, -3468,
      -3463, -3465, -3472, -3470, -3476, -3479, -3468, -3473, -3484, -3455, -3468, -3475, -3444, -3459, -3470, -3471,
      -3469, -3469, -3464, -3469, -3472, -3464, -3472, -3465, -3459, -3477, -3470, -3468, -3475, -3468, -3468, -3474,
      -3470, -3466, -3470, -3480, -3469, -3457, -3472, -3476, -3466, -3456, -3471, -3475, -3468, -3472, -3471, -3470,
      -3468, -3458, -3468, -3481, -3480, -3470, -3456, -3452, -3465, -3475, -3480, -3472, -3474, -3474, -3484, -3480,
      -3471, -3480, -3480, -3473, -3467, -3465, -3464, -3458, -3461, -3465, -3465, -3475, -3483, -3476, -3479, -3479,
      -3472, -3473, -3477, -3473, -3472, -3481, -3473, -3465, -3464, -3463, -3465, -3470, -3475, -3473, -3474, -3467,
      -3468, -3485, -3483, -3473, -3475, -3473, -3465, -3467, -3475, -3482, -3476, -3470, -3473, -3474, -3477, -3474,
      -3465, -3465, -3467, -3471, -3471, -3481, -3489, -3479, -3472, -3476, -3478, -3477, -3467, -3469, -3485, -3477,
      -3471, -3480, -3471, -3482, -3475, -3465, -3462, -3470, -3473, -3469, -3459, -3476, -3484, -3475, -3470, -3470,
      -3476, -3481, -3488, -3487, -3478, -3478, -3484, -3479, -3482, -3482, -3480, -3488, -3479, -3459, -3469, -3480,
      -3486, -3472, -3457, -3455, -3476, -3492, -3474, -3465, -3475, -3479, -3474, -3478, -3490, -3488, -3482, -3482,
      -3478, -3478, -3482, -3471, -3473, -3470, -3463, -3474, -3473, -3454, -3463, -3484, -3479, -3481, -3475, -3484,
      -3475, -3476, -3476, -3460, -3461, -3466, -3465, -3465, -3469, -3481, -3481, -3475, -3476, -3474, -3484, -3482,
      -3480, -3484, -3482, -3478, -3478, -3487, -3463, -3458, -3479, -3472, -3468, -3457, -3463, -3481, -3478, -3478,
      -3467, -3478, -3490, -3478, -3476, -3475, -3469, -3480, -3485, -3470, -3469, -3476, -3466, -3464, -3466, -3476,
      -3478, -3487, -3477, -3488, -3490, -3486, -3468, -3462, -3479, -3475, -3459, -3457, -3469, -3466, -3468, -3473,
      -3476, -3473, -3478, -3489, -3490, -3484, -3474, -3482, -3492, -3470, -3470, -3472, -3473, -3477, -3474, -3478,
      -3480, -3470, -3472, -3479, -3474, -3486, -3490, -3488, -3489, -3480, -3468, -3476, -3481, -3478, -3475, -3477,
      -3472, -3472, -3474, -3478, -3473, -3484, -3495, -3486, -3477, -3488, -3494, -3493, -3488, -3484, -3486, -3484,
      -3480, -3479, -3477, -3476, -3487, -3481, -3473, -3479, -3473, -3470, -3479, -3474, -3476, -3482, -3480, -3482,
      -3485, -3488, -3490, -3487, -3477, -3483, -3494, -3490, -3479, -3488, -3499, -3490, -3484, -3482, -3483, -3480,
      -3475, -3472, -3476, -3475, -3477, -3479, -3475, -3473, -3476, -3483, -3491, -3489, -3479, -3480, -3488, -3489,
      -3491, -3486, -3488, -3484, -3475, -3487, -3502, -3498, -3481, -3476, -3490, -3499, -3487, -3476, -3484, -3480,
      -3475, -3480, -3496, -3497, -3480, -3476, -3478, -3483, -3488, -3484, -3492, -3496, -3490, -3497, -3494, -3487,
      -3497, -3498, -3493, -3491, -3497, -3500, -3493, -3494, -3496, -3490, -3488, -3485, -3490, -3491, -3492, -3489,
      -3486, -3487, -3485, -3488, -3493, -3495, -3494, -3486, -3486, -3488, -3494, -3499, -3503, -3499, -3495, -3485,
      -3492, -3509, -3503, -3494, -3504, -3496, -3491, -3496, -3490, -3498, -3493, -3482, -3481, -3498, -3500, -3496,
      -3494, -3502, -3497, -3499, -3495, -3485, -3480, -3490, -3494, -3496, -3488, -3494, -3501, -3512, -3501, -3496,
      -3510, -3511, -3493, -3494, -3487, -3501, -3502, -3467, -3483, -3506, -3500, -3491, -3501, -3483, -3490, -3495,
      -3500, -3495, -3492, -3493, -3490, -3496, -3499, -3481, -3482, -3492, -3488, -3498, -3499, -3496, -3492, -3488,
      -3495, -3494, -3483, -3491, -3492, -3495, -3491, -3489, -3499, -3492, -3479, -3482, -3486, -3479, -3489, -3501,
      -3493, -3489, -3489, -3488, -3485, -3481, -3495, -3495, -3486, -3489, -3496, -3494, -3496, -3493, -3491, -3488,
      -3499, -3500, -3492, -3491, -3490, -3498, -3495, -3495, -3497, -3487, -3485, -3488, -3482, -3485, -3487, -3487,
      -3483, -3496, -3503, -3495, -3482, -3489, -3492, -3501, -3511, -3491, -3495, -3500, -3490, -3492, -3497, -3504,
      -3503, -3497, -3493, -3494, -3489, -3495, -3492, -3483, -3493, -3493, -3490, -3490, -3504, -3499, -3501, -3499,
      -3487, -3496, -3501, -3497, -3493, -3492, -3491, -3492,
    ];
    let wl = wav.len();
    let wlh = (wl >> 8) as u8;
    let wll = (wl & 0xff) as u8;

    let expected_x3_output: &[u8] = &[
      // Frame header
      b'x', b'3', // "x3"
      1, 1, // Source Id, Num Channels
      wlh, wll, // Num samples
      2, 144, // Num encoded bytes
      0, 0, 0, 0, 0, 0, 0, 0, // Time
      110, 210, // Header CRC
      61, 223, // Payload CRC
      // Frame payload
      242, 123, 202, 56, 106, 202, 124, 8, 122, 249, 136, 173, 202, 23, 69, 105, 50, 133, 201, 145, 251, 206, 83, 125,
      159, 181, 181, 187, 83, 151, 166, 35, 77, 194, 163, 77, 162, 57, 40, 226, 8, 249, 137, 153, 184, 188, 231, 226,
      74, 164, 250, 75, 2, 173, 171, 81, 11, 146, 16, 151, 64, 78, 179, 117, 69, 230, 20, 39, 28, 191, 212, 21, 239,
      155, 225, 221, 90, 73, 32, 134, 219, 114, 144, 74, 204, 87, 55, 217, 203, 138, 89, 40, 48, 36, 104, 193, 141,
      106, 121, 6, 160, 84, 138, 16, 129, 251, 192, 5, 233, 255, 97, 121, 123, 68, 107, 242, 0, 123, 161, 178, 194,
      235, 2, 232, 33, 190, 82, 201, 29, 71, 157, 59, 212, 168, 215, 141, 41, 79, 59, 209, 74, 156, 82, 211, 249, 11,
      215, 104, 187, 178, 41, 7, 90, 202, 16, 213, 231, 54, 76, 177, 137, 152, 247, 76, 195, 228, 133, 27, 48, 193, 10,
      104, 2, 2, 44, 12, 24, 16, 52, 195, 2, 101, 225, 124, 85, 63, 208, 133, 125, 86, 222, 78, 194, 57, 228, 179, 117,
      2, 209, 177, 202, 100, 250, 68, 37, 121, 131, 100, 237, 199, 184, 64, 89, 197, 181, 138, 68, 47, 153, 151, 213,
      45, 87, 122, 20, 163, 108, 137, 33, 165, 167, 187, 117, 146, 84, 36, 99, 30, 141, 92, 56, 185, 90, 145, 218, 45,
      208, 190, 89, 36, 47, 179, 236, 141, 178, 149, 218, 115, 173, 57, 148, 214, 246, 73, 68, 38, 102, 123, 136, 228,
      18, 134, 23, 144, 171, 110, 47, 78, 73, 162, 187, 159, 47, 227, 175, 225, 0, 47, 126, 145, 192, 227, 92, 56, 92,
      238, 133, 161, 30, 89, 194, 6, 249, 134, 13, 247, 125, 86, 102, 22, 38, 140, 24, 129, 37, 164, 139, 130, 208, 81,
      48, 230, 121, 76, 223, 130, 222, 237, 135, 79, 162, 137, 32, 138, 234, 210, 156, 151, 72, 78, 25, 120, 69, 189,
      172, 102, 243, 188, 84, 95, 193, 111, 49, 223, 116, 101, 143, 33, 153, 37, 26, 227, 57, 250, 131, 2, 94, 82, 161,
      105, 109, 20, 40, 29, 11, 232, 1, 112, 74, 155, 107, 209, 199, 133, 94, 9, 19, 176, 190, 11, 230, 129, 12, 0,
      128, 120, 95, 193, 143, 168, 41, 239, 88, 189, 176, 146, 166, 2, 37, 53, 30, 124, 50, 112, 188, 193, 124, 8, 64,
      55, 96, 90, 199, 16, 190, 174, 12, 2, 245, 208, 192, 186, 97, 169, 29, 77, 51, 44, 187, 36, 195, 109, 1, 140, 9,
      20, 68, 2, 208, 63, 187, 244, 62, 245, 252, 98, 184, 217, 96, 62, 255, 254, 251, 50, 218, 163, 255, 36, 161, 15,
      79, 117, 171, 139, 245, 26, 105, 69, 26, 186, 82, 47, 17, 89, 70, 236, 33, 108, 226, 85, 107, 203, 221, 190, 49,
      102, 160, 236, 147, 74, 102, 79, 43, 190, 174, 209, 154, 215, 225, 156, 190, 109, 86, 130, 143, 26, 128, 165, 11,
      36, 65, 207, 66, 180, 143, 144, 164, 90, 82, 135, 251, 27, 206, 46, 85, 139, 140, 5, 176, 34, 16, 126, 33, 100,
      97, 180, 1, 93, 173, 62, 132, 24, 208, 150, 20, 89, 10, 75, 30, 163, 171, 9, 180, 99, 213, 104, 49, 69, 253, 144,
      182, 154, 114, 133, 141, 252, 151, 240, 252, 191, 163, 225, 164, 48, 158, 196, 188, 251, 246, 20, 31, 240, 122,
      244, 50, 75, 65, 115, 200, 67, 104, 231, 206, 163, 11, 220, 43, 125, 197, 158, 66, 34, 185, 2, 224, 173, 110, 95,
      217, 198, 201, 21, 79, 232, 179, 51, 89, 183, 8, 196, 180, 129, 77, 210, 75, 4, 122, 76, 180, 182, 152, 137, 86,
      190, 40, 184, 232, 22, 171, 193, 4, 165, 8, 170, 144, 0,
    ];
    let x3_output: &mut [u8] = &mut [0u8; NUM_SAMPLES * 2];
    let valid_len = {
      let writer = &mut SliceByteWriter::new(x3_output);
      let params = &Parameters::default();

    // make stream encoder with 1 channel and default block length
      let mut encoder : StreamEncoder<_, 1, {x3::Parameters::DEFAULT_BLOCK_LENGTH}> = StreamEncoder::new(writer, params);
      let mut wav_iter = wav.iter();
        
      // stream some samples
      encoder.process_interleaved(wav_iter.by_ref().take(21)).unwrap();
      encoder.process_interleaved(wav_iter.by_ref().take(3)).unwrap();
      encoder.process_interleaved(wav_iter.by_ref().take(44)).unwrap();

      // stream remaining samples
      let _ = encoder.process_interleaved(wav_iter);
        
      // close stream encoder
      let _ = encoder.close();

      writer.stream_position().unwrap() as usize
    };//writer scope
    assert_eq!(&expected_x3_output[..valid_len], &x3_output[..valid_len]);
  }


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
      
      // make stream encoder with 1 channel and default block length
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