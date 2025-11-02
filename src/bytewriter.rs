use crate::error::{Result, X3Error};

#[cfg(not(feature="std"))]
pub enum SeekFrom{
    Start(u64),
    End(i64),
    Current(i64),
}

///
/// Generic trait with all functions required to write to underlying seekable memory
/// structure
/// 
pub trait ByteWriter {
    fn align<const N: usize>(&mut self)-> Result<usize>;
    // Writing
    fn write_all(&mut self, value: impl AsRef<[u8]>) -> Result<()>;
    fn flush(&mut self)-> Result<()>;
    // seeking
    fn seek(&mut self, pos: SeekFrom)-> Result<u64>;
    fn stream_position(&mut self)-> Result<u64>;
}

///
/// Wrapper struct implementing ByteWriter trait to an underlying memory slice
/// 
pub struct SliceByteWriter<'a> {
    slice: &'a mut [u8],
    p_byte: usize,
    stream_length: usize,
}

impl<'a> SliceByteWriter<'a> {
    pub fn new(slice: &'a mut [u8])-> Self{
        SliceByteWriter {
            slice,
            p_byte: 0,
            stream_length: 0,
        } 
    }
}

impl<'a> ByteWriter for SliceByteWriter<'a> {
    fn align<const N: usize>(&mut self) -> Result<usize> {
        let residual = self.p_byte % N;
        if residual == 0 {
            /* Nothing to do */
            return Ok(0);
        } 
        let zero_array = [0u8; N];
        self.write_all(&zero_array[residual..])?;
        Ok(N-residual)
        
    }

    fn flush(&mut self)-> Result<()> {
        Ok(())
    }

    fn seek(&mut self, pos: SeekFrom)-> Result<u64> {
        let abs_pos = match pos {
            SeekFrom::Current(pos) => {
                ((self.p_byte as i64) + pos) as usize
            },
            SeekFrom::Start(pos) => {
                pos as usize
            },
            SeekFrom::End(pos) => {
                (self.stream_length as i64 + pos) as usize
            }
        };
        if abs_pos > self.slice.len() {
            return Err(X3Error::ByteWriterInsufficientMemory)
        }
        self.p_byte = abs_pos;
        if self.p_byte > self.stream_length {
            self.stream_length = self.p_byte;
        }
        Ok(self.p_byte as u64)
    }

    fn stream_position(&mut self)-> Result<u64> {
        return Ok(self.p_byte as u64);    
    }

    fn write_all(&mut self, value: impl AsRef<[u8]>) -> Result<()> {
        let value = value.as_ref();
        if value.len() > self.slice[self.p_byte..].len() {
            return Err(X3Error::ByteWriterInsufficientMemory);
        } self.slice[self.p_byte..self.p_byte + value.len()]
            .copy_from_slice(value);

        self.p_byte += value.len();
        if self.p_byte > self.stream_length {
            self.stream_length = self.p_byte;
        }

        Ok(())
    }
}


#[cfg(feature = "std")]
pub use stream_byte_writer::*;
#[cfg(feature = "std")]
pub mod stream_byte_writer{
    pub use std::io::{Write, Seek, SeekFrom};
    use crate::bytewriter::ByteWriter;
    use crate::error::X3Error;

    ///
    /// Wrapper Struct implementing ByteWriter trait for any underlying Seek + Write stream 
    /// (e.g. io::File, io:BufWriter, io::Cursor, etc...)
    /// 
    pub struct StreamByteWriter<'a, W> 
        where W: Write + Seek
    {
        writer: &'a mut W,
    }

    impl<'a, W> StreamByteWriter<'a, W> 
    where W: Write + Seek
    {
        pub fn new(writer:&'a mut W) -> Self {
            StreamByteWriter {
                writer,
            }
        }
    }

    impl<'a, W> ByteWriter for StreamByteWriter<'a, W> 
        where W: Write + Seek
    {
        fn align<const N: usize>(&mut self)-> crate::error::Result<usize> {
            let position = self.writer.stream_position().map_err(X3Error::from)?;
            let residual = (position as usize) % N;
            if residual == 0 {
                /* Nothing to do */
                return Ok(0);
            } 
            let zero_array = [0u8; N];
            self.write_all(&zero_array[residual..])?;
            Ok( N - residual)
        }

        fn flush(&mut self)-> crate::error::Result<()> {
            self.writer.flush().map_err(|e| X3Error::Io(e))
        }

        fn seek(&mut self, pos: SeekFrom)-> crate::error::Result<u64> {
            self.writer.seek(pos).map_err(X3Error::from)
        }

        fn stream_position(&mut self)-> crate::error::Result<u64> {
            self.writer.stream_position().map_err(X3Error::from)
        }

        fn write_all(&mut self, value: impl AsRef<[u8]>) -> crate::error::Result<()> {
            let value = value.as_ref();
            self.writer.write_all(value).map_err(X3Error::from)?;
            
            Ok(())
        }
    }
}
